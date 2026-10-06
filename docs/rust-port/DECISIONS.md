# Decisions the spec did not cover

Choices made while executing the roadmap, where SPEC.md and ROADMAP.md were
silent. Each one took the option that best preserves parity (ROADMAP working
rule 4). The owner can overturn any of them; add a line saying so.

Format: number, date, work package, the decision, why.

## D1. Hash maps are banned outright in the simulation crates

2026-10-03, WP 0.1. SPEC 3.2 forbids hash-map *iteration* in `mp_math`,
`mp_track`, `mp_levels` and `mp_sim`. `check-deps` forbids the types
(`HashMap`, `HashSet`) instead: a source scan cannot tell lookup from
iteration, and SPEC 4.2 wants JS insertion order, which a vector of pairs or
an index-keyed array gives and a hash map never does.

## D2. `check-deps` also scans for platform float functions

2026-10-03, WP 0.1. SPEC 4.2 says the simulation and world generation never
call `f64::sin` and friends. `check-deps` enforces it by scanning the
sources of `mp_math`, `mp_track`, `mp_levels`, `mp_sim`, `mp_worldgen` (and
the kernel's wasm crate) for `x.sin(`, `f64::sin(` and the like: every
inexact function, plus `powi` (not the same rounding as `Math.pow`) and
`mul_add` (fuses a rounding away). Comments are skipped. `mp_canvas`,
`mp_audio` and `mp_game` are not scanned: their output is compared with
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

Note (2026-10-04): `-Oz` against `-O3`, `-O4` and `--converge` was
measured with frames to time in D673: `-Oz` stays.

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
clock. The e2e harness adds `MP_QUERY` to every page, so the whole suite runs
with any hook on (`MP_QUERY=kernel=1 npm run test:e2e`).

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
`crates/mp_scene/FORMAT.md`.

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
Rust `mp_audio` takes one random stream and draws from it exactly where
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
log. M5 plays the same facade log into `mp_audio` with the null backend
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

## D50. The shape of `mp_math`'s port of `util/math.js`

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
`mp_math::js` over the same inputs on both sides (edge values, then
mulberry32 spreads; noise also at coordinates past 2^31, where `i & 255`
wraps through ToInt32), stored as FNV-1a 64 of the f64 bits plus the first
16 values in hex. NaN is written canonically on both sides, because V8 keeps
whatever NaN bits an operation left (`Math.max(NaN, 1)` stores
0xfff8000000000000). The golden is compiled into the test, so it runs in
wasm too.

## D52. Index loops stay index loops

2026-10-03, WP 1.2. Clippy's `needless_range_loop` is allowed in
`mp_track`, `mp_levels` and `mp_sim`. A JS `for (let k = 0; k < n; k++)`
that reads several arrays at `k` is ported as the same loop, so the port can
be read beside the JS line for line; rewriting it as iterator chains hides
the correspondence and invites reordering.

## D53. Seaside's survey file

2026-10-03, WP 1.2. `tools/seaside/build.py` now also writes
`assets/seaside/survey.bin` (format in its `Survey` docstring): every value
`circuit.js` and `ground.js` hold, from the same Python values, with each
grid's zlib stream as it is (the JS file holds it in base64). Rerunning the
script from its cache reproduces both JS modules byte for byte, so the binary
was written by a real run, not converted from the JS. `mp_levels::survey`
decodes it the way `load.js` does, including where `load.js` stores into a
`Float32Array` (grid values, the blend's scratch buffers) and where into a
plain array (the samplers' results). `tools/parity/seaside-golden.mjs`
writes `parity/golden/seaside/survey.json` (the decoded line, grids and
features, and the four samplers at 33,000 points); the Rust matches it bit
for bit. The photo stays in `src/levels/seaside/` until world generation
needs it (M7).

## D54. Level functions are shared closures; Seaside is prepared

2026-10-03, WP 1.2. A JS level carries functions (`elevation`, `ground`,
`looseGround`, `loop.path`). `mp_track::Level` holds them as
`Arc<dyn Fn ... + Send + Sync>`, so a level is plain to clone and share. As
in the JS, `seaside::level()` is the menu's level without survey data, and
building its Track fails until `seaside::prepare(level, data)` has filled in
the path and the ground. `mp_levels` does not embed the survey: the caller
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
checked against `CAR_SPECS` in `mp_sim`'s tests (WP 1.3). Each is ported in
the package that brings what it needs.

## D57. The shape of `mp_sim::physics` and `vehicle`

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

## D58. Module traces are replayed by `mp_sim::staged`

2026-10-03, WP 1.3. The module oracle's staging (`tools/parity/lib/node-sim.mjs`)
is ported as `mp_sim::staged`, which grows with the work packages; the test
`crates/mp_sim/tests/module_phys.rs` transcribes each scenario's input
function from the catalogue in `sim-module.mjs`. The trace record is written
from views of the state (`mp_sim::trace`), so it can follow the JS layout
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
(`mp_sim::rng`), seeded as `simStreams` seeds them.

## D60. Race rules: one player for now, events, hash

2026-10-03, WP 1.5. `mp_sim::race` ports Race.update's simulation in its
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

2026-10-03, WP 1.6. `mp_sim::pursuit` ports Pursuit.js, `mp_sim::police`
PoliceDriver.js. Every body has a `BodyId` (pool and index; police are units
then roadblock cars, as the trace counts them), carried in its `AgentView`
with the `police` and `kinematicOnly` flags the drivers test. A unit's
target is a racer's index in `racers` (player first, then the rivals); the
pursuit reaches the racers' bodies through a small `Racers` trait (their
pose, a rival's finish, holding a rival, spiking a tyre), so the race, the
module staging and the tests each supply their own. `mp_sim::field` holds
what the race and the staging share: the agent list in collision order,
live views, and the collision pass over all five pools. The simulation
half of PursuitView (damage from hits and walls with the half-second
`lastHit` per body, wrecks, holds and `holdControls`, the PIT yaw kick, and
the effects of its events: release onto the road, the barrier's slowdown,
a bust's crash) is `race::PursuitView`, applied at SPEC 4.3's step 17. The
pursuit's events reach the client as `SimEvent::Pursuit`. Pursuit.js's
`hud()` and `propMark()` are presentation and stay with the client (M8).

## D62. The `mp-sim` runner lives outside `src/`

2026-10-03, WP 1.8. `mp-sim` (`crates/mp_sim/bin/mp-sim.rs`) reads the
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
are off; `mp_game` turns on the 3D renderer (`bevy_render`,
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
`mp_scene::cache` computes `tools/parity/lib/jstree.mjs`'s key in Rust, the
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
(`crates/mp_game/src/tint.wgsl`) multiplies the base colour by them, so
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
`pitch` -0.08), on the Rust `Track` from `mp_track` and `mp_levels`;
`flyCamera` is ported line for line, with `dt` clamped to 1/20 s as the JS
frame loop does. Without `s` the client runs the menu's attract camera
(16 m/s along the first zone from s = 120, h 7, back 22, lat 3, pitch
-0.05). Up and Down change the fly camera's speed by 10 m/s, a convenience
the JS does not have. The sky, fog and lights are the export's, captured at
the route's start (D24), and do not follow the route's time of day yet: on
the sprint levels the light at the far end differs from the JS until the
sky is ported (WP 2.3 and M3).

## D106. The web page, the gesture bridge and the test hooks

2026-10-03, WP 2.1. `crates/mp_game/web/index.html` checks
`navigator.gpu.requestAdapter()` before downloading anything (asking up to
four times: headless Chrome answers null while its GPU process starts) and
shows a plain "no WebGPU" page naming the browsers to use. It downloads the
wasm and the scene with progress, starts the app before the scene arrives
(so the first frame is early), and forwards pointer-up, touch-end, click and
key-down to the wasm's `gesture()` inside the handler; the wasm side only
counts them for now (audio, fullscreen, the landscape lock and the motion
permission arrive in M5 and M6). The wasm publishes its state on
`window.__mp` (`state`, `progress`, `ready`, `frames`, `firstFrameMs`,
`readyMs`, `counts`, ...), and `?stats=1` shows a panel with the frame rate
and worst frame measured as the JS game's panel does.
High quality (shadows on) defaults as in the JS: on, except on touch
devices; `?hq=0` or `?hq=1` overrides it. The shadow map is one cascade
to 140 m (about the JS's ±70 m box) at 2048², with Bevy's own biases.
`__mp.screenshot(name)` saves the next frame through Bevy's screenshot as
a download: headless Chrome does not composite a WebGPU canvas into its own
screenshots. Headless Chrome on the dev machine gets the hardware adapter
with `--enable-unsafe-webgpu --enable-features=Vulkan --use-angle=vulkan
--ignore-gpu-blocklist`. Scenes over about 100 MB cannot be answered
through the harness's request interception (the tab dies), so the larger
levels are checked through the registered server.

## D107. The dev server sends precompressed files under `dist/`

2026-10-03, WP 2.1. `cargo xtask web --release` writes `mp_game_bg.wasm.gz`
and `mp_game.js.gz`; `tools/serve.py` sends the `.gz` with
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

## D131. `BufferGeometry`: attributes in insertion order, typed arrays as `mp_scene::BufferData`

2026-10-03, WP 3.1. Attributes are a vector of (name, attribute) with a JS
object's order: setting an existing name keeps its place, deleting and
setting again moves it to the end. The order is observable:
`mergeGeometries` follows the first geometry's, and `LatheGeometry` sets
`uv` before `normal`. An attribute's array is an `mp_scene::BufferData`, so
geometry goes into a scene as it is (`to_mesh_desc`, `add_to_scene`);
writes behave as stores into the JS typed array (`f32` rounding, integer
wrap through ToInt32/ToUint32, three's `normalize`/`denormalize` for a
normalized attribute). The index is a `BufferAttribute` too: `set_index`
with a list picks `Uint16` or `Uint32` by three's `arrayNeedsUint32`, and
`set_index_attribute` takes one as built (`TerrainMesh.js` and `Sea.js`
choose the type themselves). Groups are `usize` start, count and material
index. `toNonIndexed` on a geometry without an index returns a copy where
three warns and returns `this`. Nothing was added to `mp_scene`.

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
(D51). `crates/mp_worldgen/tests/three_geom.rs` rebuilds every case and
requires all of it to match. The SPEC's bar is 1e-6; the result is
bit-identical in every case, native and in wasm
(`cargo test --target wasm32-unknown-unknown -p mp_worldgen`). CI checks
that the golden regenerates and runs mp_worldgen's tests in wasm.

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
style the game requests, so mp_canvas has no synthetic bold (a synthetic
oblique, Skia's 0.25 skew, is there for the gallery). `FontBook::bundled()`
compiles the files in (about 4 MB, most of it Arimo's and Rajdhani's
non-Latin coverage: subsetting both sides to the characters in use is the
obvious size fix once the owner has chosen). The alternatives for the
gallery are fetched on demand into `target/font-candidates/`, not
committed. The owner picks from `/parity/report/fonts/`.

## D151. Exact-area coverage and own compositing, on tiny-skia's paths

2026-10-03, WP 3.2. mp_canvas uses tiny-skia for the pixmap, path building
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

## D155. The shape of `mp_worldgen::textures`

2026-10-03, WP 3.2. One function per JS generator, drawing on mp_canvas in
the JS order, returning a `Texture` (unpremultiplied RGBA as uploaded, the
source kind, wrap, colour space, anisotropy) with `desc()` for an
`mp_scene::TextureDesc` (canvas textures flip on upload, data textures do
not). The JS module's `Map` cache is a `TextureCache` owned by the world
build, with the JS keys, including their quirk: a sign's key leaves out its
font and border, so two signs differing only in those share a texture.
Stores keep their array types: `Uint8ClampedArray` (ImageData) rounds half
to even and clamps (`ImageData::set`), `Uint8Array` (the DataTexture)
truncates, `Float32Array` lattices and noise go through `fround`. The
noise textures are bit-identical to the JS.

## WP 3.3 decisions

## D190. The shape of the builders' API

2026-10-03, WP 3.3. Each JS module is a module of `mp_worldgen`:
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
world data (`SimWorldData`, `mp_levels::world::WorldData`, filled by the
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
`crates/mp_worldgen/tests/builders.rs` makes the same calls and is
bit-identical in every case, native and in wasm. CI checks that the golden
regenerates.

## WP 5.1–5.2 decisions

## D210. The facade: handles like the JS objects, one `Op` per call

2026-10-03, WP 5.1. `mp_audio::wa` ports call for call: `ctx.create_gain()`,
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
`Float32Array`, every inexact function through `mp_math::kernel`);
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

2026-10-03, WP 3.4. `Terrain.js` is `mp_worldgen::terrain`, the
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
an `mp_scene::Scene` through the object tree, each mesh's `mp_scene` digest
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
box. `mp_worldgen` has no JPEG decoder, and the client already has one
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
(`crates/mp_game/src/render/material.rs`), whose WGSL
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
`render::sky::SkyState` from `mp_levels`' keys (colours through three's
sRGB-to-linear with the kernel's `pow`, trigonometry through
`mp_math::kernel`), driving the dome's uniforms, the sun or moon, the
hemisphere light, the fog and the exposure from the fly or attract
camera's s, with `?t=` and `?freeze=1` as in the JS. A test checks the
result at s = 0 against every export's dome uniforms, fog and exposure, to
1e-12. WP 3.5 ports `Sky.js` into world generation; this moves there then.
`world.nightMaterials` follow the sky's night factor from the export's
`night_params` (all `emissiveIntensity`). The client's Track takes the
scenery's runout from `mp_levels::world` (as the simulation does), so the
fly camera reaches the end of the road as the JS one does (stations past
`length` on Sierra and Coast were wrong without it). The models scene, which
has no dome, takes Sierra's sky at the start and the dome's noise texture
from `mp_worldgen`'s `terrainDetailTexture`; it draws on black.

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
`mp_worldgen::three_geom`; a kind scene takes its material from the level's
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
`application/wasm`), waits for `__mp.ready` and saves `__mp.screenshot`; it
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
in `mp_worldgen::road`; `Sky.js` is `sky`, `Sea.js` is `sea`, and the
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
scenery reads them, so mp_track's `Track` is left alone; the same goes for
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

## D310. `valley/flora.js` is `mp_worldgen::flora`, ported once for its three users

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

2026-10-03, WP 3.6. `Mountain.js` is `mp_worldgen::mountain`: `mod.rs`
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
`mp_worldgen::scenery` reads the three goldens into a `PlanRecording`
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

## D331. The shape of `mp_worldgen::valley`, and the L3 gate for zone 1

2026-10-03, WP 3.7. `Valley.js` is `mp_worldgen::valley` (a directory:
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
pixel has alpha between 1 and 31) where mp_canvas gives exact area
coverage (D151), and the unpremultiplied colour of a faint edge pixel that
Chrome leaves transparent counts in full. Premultiplied, RGB is within
(2.45/2.65/1.12), alpha is not (4.20); 1.4 % of pixels fall on the other
side of the material's `alphaTest` 0.45. Reproducing Chrome's multisampled
stroke coverage belongs to mp_canvas (WP 3.2's owner), so the test holds
the corn strip to 6 levels until then and reports the numbers; every other
Valley texture is within the gate.

**Resolved 2026-10-04 (D650-D653).** mp_canvas now strokes as Chrome's
GPU canvas does, and the corn strip is within 0.00/0.00/0.01/0.00 levels
of the capture (from 4.56/4.95/2.04/4.20); `tests/valley.rs` holds it to
the gate of 3 like every other Valley texture. The tassels, lone lines
drawn after the strip's first five curves, are multisampled (D653).

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
a real race: `mp_sim`'s `LevelRuntime` and `SimState` for the level
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
and the radio's 700 ms wait. `mp_audio::timers` is the reference's
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
`Music.js` and `tracks.js` are ported faithfully (`mp_audio::music`,
`mp_audio::tracks`), as is `RadioVoice.js` with `clipId`
(`mp_audio::radio`, fetching through a `Fetch` trait). The sirens, radio
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

2026-10-03, WP 5.3. `mp_audio::game::GameAudio` keeps `Audio.js`'s methods
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

## D350. The shape of `mp_worldgen::city`

2026-10-03, WP 3.8. `City.js` is `mp_worldgen::city` (`city/mod.rs`),
`city/freeway.js` is `city::freeway`, `city/cityTextures.js` is
`city::textures`; `city/geom.js` stays `mp_worldgen::geom` (WP 3.3).
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
`world.oppositeCarriageway`; its height is `mp_levels::world::opp_y`, which
the freeway uses too (`freeway::opp_y`). `tests/city.rs` checks the runout,
the carriageway and `oppY` at the dumped samples against `mp_levels::world`
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
draws text in whatever faces that machine's Chrome falls back to, mp_canvas
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
`cargo test -p mp_worldgen --test city` (and in wasm).

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
Subsetting changes no pixel: the gallery drawn by mp_canvas from the
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

2026-10-03. `crates/mp_canvas/build.rs` reads `assets/fonts/fonts.json`
(with a small JSON reader of its own: the crate takes no build
dependencies) and generates `BUNDLED`, `BUNDLED_GENERIC`,
`BUNDLED_FALLBACK` and the files' bytes, each file included once even when
two families use it (Arial and Arial Black share Roboto's bytes). Changing
the fonts is a change to `fonts.json` and the files beside it; then
`node tools/parity/textures.mjs` and `node tools/parity/mountain-textures.mjs`
recapture the references, and mp_canvas's `tests/text.rs` needs Chrome's
`measureText` numbers again. With Roboto, every texture and probe stays
within the gate (largest mean absolute difference 0.87 of 255, gravel,
which has no text; largest for text 0.53, the text-faces probe; Mountain's
lettered canvases at most 0.74).


## WP 4.1 decisions

## D410. The shape of `mp_worldgen::car_model`; the module's caches live on the graph

2026-10-03, WP 4.1. `vehicles/CarModel.js` is `mp_worldgen::car_model`, a
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
mp_worldgen does not depend on mp_sim (SPEC 3.2); `tests/car_model.rs`
takes it as a dev-dependency (check-deps follows normal edges only) and
requires every kind's `dims`, built at high and at low detail, to equal
`mp_sim::dims::dims(kind)` and the JS dump that table was checked against
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

## D430. The loopback session lives in the client until `mp_net` has one

2026-10-03, WP 4.2. SPEC 3.3's `session.advance(frame_dt, input)` is
`mp_game::play::session::Session`: it owns the `LevelRuntime`, the state
before the last tick (`prev`) and after it (`curr`), and the time not yet
stepped. A frame adds its time, clamped to 1/20 s as the JS frame loop
does, and runs every tick that has come due, at most six (SPEC 4.1; with a
1e-9 s tolerance so 1/20 s is six ticks whatever the rounding); beyond
that the race slows down. Before each tick it asks for that tick's
quantised `InputFrame`, so the input layer runs at the tick rate (SPEC
8.2), and it copies `curr` into `prev` (`clone_from`, a few kilobytes) so
the client draws `prev`→`curr` at `alpha = acc / DT`. The events of the
frame's ticks are kept in order for the client. `mp_net` is still empty;
when multiplayer (M10) gives it a session type the loopback becomes one
of its transports and this module goes there. A test steps a session with
ragged frame times (60, 144, 30 Hz, zero, a stall) beside a direct
`mp_sim` loop with the same autopilot and requires the same tick count
every frame and the same final hash: no drift.

## D431. Bevy UI for the plain HUD and the touch controls

2026-10-03, WP 4.6. The race's text (countdown, GO!, toasts, position,
clock, lap, speed and gear, the pause card and the results list) and the
touch controls are Bevy UI nodes, as SPEC 8.1 says the UI will be.
`mp_game` turns on Bevy's `bevy_ui`, `bevy_ui_render`, `bevy_text` and
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
pointer with touch points) into `__mp.touch` and measures
`env(safe-area-inset-*)` into `__mp.insets`; the controls show only while
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
a direct `mp_sim` run of the same ticks. `midnight-racer --level <id>
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
(`__mp.lateFrames`, a warning in the log) so a missed combination shows;
none in the runs recorded in BASELINE.md.

Render pipelines only are counted as waiting: on WebGL2 Bevy 0.19 queues
its "sparse buffer update" compute pipeline although the device has no
compute, and that pipeline waits for ever for a shader that is never
loaded (the client has no compute pipelines of its own).

## D391. The WebGL2 build is the `mp_webgl2` cfg in its own target directory

2026-10-03, WP 2.7, SPEC 2. Bevy picks its backend with a Cargo feature
(`webgpu` overrides `webgl2`), so the two web builds need different Bevy
features. Cargo features of `mp_game` would have to name `bevy/webgpu`,
which also reaches the native build's `bevy` (one dependency, unified):
every native build cache in every worktree would be rebuilt for a feature
that does nothing natively; and Cargo refuses the same crate twice under
two names, so a wasm-only alias cannot carry them. Instead the wasm
dependency tables are split on a cfg: `cfg(all(target_arch = "wasm32",
not(mp_webgl2)))` asks for `webgpu`, `cfg(all(target_arch = "wasm32",
mp_webgl2))` for `webgl2`, and the WebGL2 build sets `RUSTFLAGS="--cfg
mp_webgl2"` (added to whatever flags the caller has; CI's `-D warnings`
stays). Because changing RUSTFLAGS invalidates a build, the WebGL2 build
has its own target directory, `target/webgl2/`, and neither build throws
the other's cache away. The native build is unchanged.

`cargo xtask web [--release]` builds both into `dist/next/`:
`mp_game.js` and `mp_game_bg.wasm` (WebGPU), `mp_game_webgl2.js` and
`mp_game_webgl2_bg.wasm` (WebGL2), each gzipped for a release; `--only
webgpu|webgl2` rebuilds one and keeps the other's files. `build.json`
lists the backends present. `cargo xtask size` checks the larger of the
two against the 10 MB budget (a browser downloads one). CI lints the
WebGL2 build (`RUSTFLAGS="-D warnings --cfg mp_webgl2" cargo clippy -p
mp_game --target wasm32-unknown-unknown --target-dir target/webgl2`) and
builds it in `cargo xtask web --release`. The client publishes which one
it is (`__mp.backend`).

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
`__mp.reload(level)` calls it, waits for `waiting`, downloads and hands
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

## D471. The runout and the opposite carriageway are computed by the scenery's rules, in `mp_levels::world`

2026-10-03, WP 3.9. Until now `mp_levels::world::world_data(id)` held the
numbers of the WP 0.4 dump (900 and 6419..9379 on Sierra, 700 and
4865..7665 on Coast, 0..14320 on the loop), the stand-in of an unported
module replayed the runout its `plan()` was recorded leaving, and nothing
gave Coast's world build its carriageway. The simulation cannot depend on
mp_worldgen (SPEC 3.2) and should not need a world build to race, so the
parts of the scenery that decide this data are ported into
`mp_levels::world`, one implementation for both: `City` (`s_a`: the
merge tag's s0, else the zone's s0 + 220, 0 on a loop; `plan_runout`:
`max(runout, 900)` off a loop; `opposite_carriageway`: sA to the length)
and `Harbor` (`s_ws`: the `bridge-up` tag's s0, else `zoneStart` + 200,
less 40; `plan_runout`: `max(runout, 700)`; the carriageway from sWS + 40
to the length), `Streets`' `runout = 0`, and `level_world_data(level,
track)`, which runs them as `World.build` runs the modules (every `plan()`
in the order `loadScenery` makes them, then every `build()`, the last
carriageway set winning). mp_worldgen's City now takes its sA, runout and
carriageway from those functions (same values: City's gates pass
unchanged); a `RecordedScenery` computes its module's runout by the rule
and sets `sim_data.runout`, and its `build()` sets the carriageway Harbor
would, so Coast's `WorldBuild::sim_data` is complete. The recording's
runout is no longer replayed; the road test still holds `track.runout` to
it. mp_sim's `stage_level` and `LevelRuntime::new` call
`level_world_data` on the Track they build; `world_data(id)` stays (the
client calls it) and builds a Track only for a level with a City or a
Harbor. Gates: `mp_levels` `tests/world_data.rs` (every level against
`parity/golden/sim/world-data.json`: runout, roadEnd, s0, s1, lanes, dir,
oppY at the samples), mp_worldgen `tests/world_data.rs` (each level's
build: the track's and the simulation's runout, and Coast's, Streets',
Desert's and Seaside's carriageway from the stand-ins' builds; Sierra's
and the loop's from City's build stay in `tests/city.rs`), and mp_sim's
35 module traces and 11 races, unchanged.

## D472. Level 1 is held to the export as one scene

2026-10-03, WP 3.9. With Mountain, Valley and City ported, Sierra's build
needs no recording at all (`scenery_factory(None)`), and
`tests/level1.rs` compares the whole scene with the export rather than
group by group: built that way, the sky updated at the export's focus,
the updaters run once at the export's frame (dt 0) and their attribute
and instance edits written into the buffers, its `mp_scene` digest must
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
frame, and building Level 1 from mp_worldgen.

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

How (`crates/mp_game/src/render/instancing.rs`):

- **Data.** The entity carries `Instances`, an `Arc` of its stream shared
  by its material groups: per instance 20 floats (80 bytes), the world
  matrix's columns, the colour and `receiveShadow`. It goes to the GPU the
  first frame it is extracted (a `VERTEX` buffer) and the CPU copy is
  dropped, as for meshes (D101).
- **Pipeline.** `ThreeKey` gains `instanced` (a separate material asset
  from the same JS material). `ThreeMaterial::specialize` then appends a
  second vertex buffer layout, stepped per instance, at shader locations 9
  to 13 (Bevy's attributes use 0 to 7, the patch attribute 8; WebGL2's 16
  attributes are enough: 12 at most), and the def `MP_INSTANCED`. The
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

2026-10-04. `three_material.wgsl` under `MP_INSTANCED` reads the world
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
`ready` (`__mp.lateFrames` 0). (The native client reports one pipeline
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
the material as before. `mp_worldgen`'s setters are unchanged.

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

## D550. The shape of `mp_worldgen::desert`

2026-10-04, WP 7.3. `Desert.js` is `mp_worldgen::desert` (`desert/mod.rs`),
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
counts and kinds always, the whole `mp_scene` digest with the cache.
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
`cargo test -p mp_worldgen --test desert` (and in wasm).

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
it. The `mp_scene` digest does not hash those flags, so this is for the
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

## D530. The shape of `mp_worldgen::coast`; `coast/kit.js` is Mountain's kit

2026-10-04, WP 7.1. `Coast.js` is `mp_worldgen::coast` (`coast/mod.rs`):
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
carriageway the build gives the simulation to `mp_levels::world` and the
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
own values. mp_worldgen makes them with three's defaults (0.00025, 1,
2000, white), as the JS material is made; the gate does what the renderer
does, from the export's fog (`coast.json`), before comparing. The client
sets them from the sky's frame each frame, as it does its own fog.

## D533. The shape of `mp_worldgen::beach`; the parts' random defaults

2026-10-04, WP 7.1. `Beach.js` is `mp_worldgen::beach` (`beach/mod.rs`),
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
mp_canvas takes the exact area (D151). Premultiplied the colour is within
2.26/5.00/1.61; 2.95 % of pixels fall on the other side of the material's
`alphaTest` 0.4. Matching Chrome's stroke coverage is mp_canvas's (WP 3.2's
owner); until then `tests/coast.rs` holds the leaf to 10 levels
unpremultiplied and 6 premultiplied and reports the numbers; every other
Level 2 texture is held to 3.

**Resolved 2026-10-04 (D650-D653).** With strokes tessellated and
multisampled as Chrome does, the leaf is within 0.011/0.025/0.015/0.002
levels (R/G/B/A, from 2.73/5.89/1.94/9.39) and no pixel crosses the
`alphaTest`; `tests/coast.rs` holds it to 3 like every Level 2 texture
(`LEAF_LIMIT` and `LEAF_PREMULTIPLIED` are gone). Chrome's coverage was
not quantised for the leaf alone: the midrib, a lone line drawn after
the leaflets, is multisampled too (D653).

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
`mp_levels::world::Harbor` as City's do (D471); no stand-in is left on
Level 2. `tests/world_data.rs` builds its levels without a road, which a
ported module's `build()` needs, so Level 2 now stops after the plans as
the City levels do, and `tests/coast.rs` checks the data from the real
build. Shared modules: `mountain::kit::world_matrices` is public (Beach's
parked cars traverse a car as `bakeStatic` does); WP 7.3's
`BufferGeometry::set_instanced_attribute` (Harbor's `aVar`, Desert's
pools) is cherry-picked unchanged. While Beach and Harbor were ported side
by side, Harbor sat behind a temporary `harbor-wip` feature; it is gone.

## D540. The shape of `mp_worldgen::harbor`

2026-10-04, WP 7.1. `Harbor.js` is `mp_worldgen::harbor` (`harbor/mod.rs`),
`harbor/build.js` is `harbor::build`, `harbor/textures.js` is
`harbor::textures`. `plan()` keeps the JS fields (z0, the span, up and down
tags, sWS, the connector's frames and gate), sets the runout by
`mp_levels::world::Harbor::plan_runout`, mirrors it into
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
form. On Sierra 15 materials get one in the first frame (at the start). A
number or colour goes to the
kind's uniform of that name if it has one animated, else to the parameter
(D411's rule); the sky's edits are left out (D493).

## D491. The client builds Level 1 for its animators; the default still draws the export

2026-10-04, WP 3.9. Animators are code (SPEC 5.1): they come only from a
world build. For a level `mp_worldgen` builds whole (Sierra, D472), the
client runs `level_jobs` itself (`crate::animate`): natively on a thread
(with `mp_worldgen`'s `parallel` feature), on the web a few jobs a frame
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
canvas textures are `mp_canvas`'s, within WP 3.2's thresholds (D312,
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
on Sierra). A node exported invisible is spawned hidden, so an animator
can show it (D498).

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
new test hook (`__mp.flyTo`, `__mp.flyQuiet`: three frames with no
pipeline compiling and the environment map built) and saves
`__mp.screenshot`; `cargo xtask parity shots` compares.

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
## D498. Level 1's scenery by name, the wasm budget, and invisible nodes

2026-10-04, WP 3.9. The client's world build named its scenery through
`scenery_factory(None)`, whose `PORTED` table links every level's scenery
into the client; with Coast, Desert, Seaside and Streets ported on main
that made the web build 10.00 MB (WebGPU) and 10.48 MB (WebGL2) after
gzip, over SPEC 6.6's 10 MB. The client now builds Sierra with its own
factory naming Mountain, Valley and City (`animate::level1_scenery`):
9.60 and 10.08 MB. Linking Level 1's world generation costs about 0.7 MB
after gzip (8.77 and 9.24 MB before WP 3.9; main's race audio and warm-up
took the rest), so the WebGL2 build is still 0.08 MB over. Not decided
here: the budget, or how the other levels' animators reach the client
(each level built in the client links its scenery). Raised with the
coordinator, who has started a size package; the owner decides.

Ways to feed a level's animators without linking its scenery build, noted
for that decision and not done: the updaters are small (City's 13 on
Sierra, a few hundred lines) and need only handles and a few numbers per
animator (positions, phases, base vertex arrays), which the build knows;
the export could carry them (an `animators` list of plain data per
updater: kind, target handles, constants), and the client run ported
updater functions over that data without the builders; or `mp_worldgen`
could split each module's updaters from its builders, so that a client
linking only the updaters builds them from a small description the
export or a build tool writes.

Nodes exported invisible (or under one) were not spawned at all, so an
animator could never show them (Cruise's cut-off hides and shows City's
chunks by distance). The loader now spawns them with `Visibility::Hidden`,
counted as before (`Counts::invisible`); the animators' visibility edits
combine each node's own flag with its ancestors' as three does. Sierra has
none; Cruise's export has 52 (486 entities instead of 432).

## D499. Coast Highway's kinds, drawn from its export

2026-10-04, after WP 7.1 (the coordinator's plan: the drawing side of the
other levels, linking no scenery; Coast's to-do from WP 7.1). Blocks of
`three_material.wgsl` as before: **Surf** (`foamMaterial`, a
ShaderMaterial: value noise, swell bands, contact foam and ragged rings;
the rings' phase from the instance's translation, the InstancedMesh being
at the origin; three's FogExp2 on it), **LighthouseBeam** (additive, its
`vV` carried interpolated as three does, its own fog fade), **Stucco**
(`tGrain` in the detail slot, a triplanar grain and a broad mottle after
`color_fragment`) and **ContainerAtlas** (the mask atlas's row by the
instance's `aVar`, the body colour from the instance colour). `aVar` is an
instance-rate attribute; the instance stream grows from 20 to 24 floats
(96 bytes) to carry a geometry's instance-rate attributes, one float each
in the geometry's order (`loader::instance_extras`, at shader location 15;
zeros without; the shadow pass reads only the matrix): the containers'
`aVar`, Desert's pools' `ph` and `fl` (D500). Sierra's 63,263 instances
take 1 MB more for it. TriplanarRock
(coast/kit.js) and the lighthouse's glow Sprite were drawn already (D494).
Without the level's animators (D498), the animated uniforms follow the
scene-wide state as the updaters set them (D293's rule: Surf's `uTime` the
clock and `uBright` lerp(1, 0.32, night), the beam's `uStrength` 0.03 +
0.32 × smoothstep(0.2, 0.8, night)); colours an updater moves on plain
materials (the bulbs, the glow's opacity and scale) and the beam's turn
stay as exported.

Gate, the web build at 1280 × 800 through the registered server
(`rust-web-stations.mjs --server`; Coast's export is over D106's
interception limit): 67 stations, 3 over the limits (13 before), median
0.26 / 0.62. The three are the beach at sunrise from the chase camera
(04000, 04250, 04500: worst 5.1 / 30.2). They predate WP 3.9's client: a
web build of 1bb2dd6 gives the same three numbers (5.16 / 30.216, 3.975 /
23.59, 2.717 / 22.243). The difference map shows the cause: the beach
road's lamp pools (`Beach.js`, additive) are lit in the client and dark in
the JS. Their `opacity` is `0.42 × smoothstep(0.08, 0.5, night)`, set by
Beach's updater; the export was taken at the route's start, at night on
Coast, so it holds 0.42, and at sunrise the JS has turned them off. A
plain material's colour or opacity moved by an updater needs the level's
animators (D498), so these three stations wait for the size decision. The material
scenes for Surf, LighthouseBeam and Stucco pass (0.06 / 0.13 at worst)
and join `all`; ContainerAtlas's scene is over (3.5 / 18.1) because the
material tool draws it without the instance stream (no `aVar`), so it
stays out of `all`; its stations (the harbour) pass.

## D500. Desert Run's kinds, drawn from its export

2026-10-04 (D556's list). **Sandstone** (strata from `tRock` in the aux
slot, the varnish from `tDetail` in the detail slot, the instance's matrix
included, as TriplanarRock), **GroundPool** (the flicker `vFl` from the
instance's `ph` and `fl` in the stream's instance-rate floats, D499, and
the clock) and **FloodBeam** (`vFace²`, the normal through the instance's
matrix as three's `mat3(instanceMatrix)`, not its inverse transpose). The
pools' and beams' opacity is a material value the updater moves with the
night (`smoothstep(0.15, 0.7, n)` and `0.13 × smoothstep(0.3, 0.8, n)`;
both 0 in the daytime export): it is one of the kind's animated values
(the block's texel 4 with the animators, D490; the night factor without),
as are the pools' `uTime` and the flicker points' clock (D293). The
material colours Desert's updater moves (flares, fires, lanterns, lamps,
bulbs, strings; `colour × k`) and the train (its sprites, its spot light)
stay as exported without the level's animators (D498). The material test
tool's overrides of a material value (`overrides.material`, the pools' and
beams' night opacity) go through `animate::fix_uniforms` as its uniform
overrides do; drawn without the instance stream, a pool has no flicker.

Gate, the web build at 1280 × 800 through the registered server: **all 63
Desert stations within the limits**, median 0.16 / 0.39, worst 0.681 mean
(02250-high) and 1.564 block 95 % (07250-chase). The material scenes for
the three pass (0.083 / 0.170 at worst) and join `all`.

## D501. Downtown Streets' kinds, drawn from its export; flat shading

2026-10-04 (D614's list). **StreetFacade** (the 4 × 4 façade atlas by
`cell`, its building's seed in the sixteens, with the raw uv's gradients;
`windowLight` per window and the street bounce from `fdata`: `cell` and
`fdata` share the patch attribute, `cell` in x and `fdata` in yzw),
**StreetAtlas** (the 4 × 3 shop-front atlas, `windowLight` × 1.4 or the
flat glow of a kind-0 cell, the faint bounce), **AmbientProp** (emissive +=
albedo × `kind_opts.rgb`, rounded to four places as the JS writes it into
its GLSL), **Neon** (`ndata`'s seed and mode: hum, buzz, switching, a
dying tube, on `uNTime`) and **Steam** (a ShaderMaterial on points: each
puff's life, rise and drift in the vertex stage, `aSeed` in the patch
attribute, a lumpy premultiplied puff in the fragment stage, from
`gl_PointCoord` with three's y; drawn with three's `CustomBlending` One,
OneMinusSrcAlpha, the one custom blending the exports use). `uNTime`,
Steam's `uTime` follow the clock and Steam's `uScale` the viewport and the
projection without the animators, as their updaters set them (D293). The
helpers `fHash`, `fHue` and `windowLight` are shared by the two atlases
(`STREET_WINDOWS`). Colours above 2 were already drawn unclamped (D452).

`flatShading` (one material on Streets, the trees; one on Sierra; three on
Coast) was ignored: the normal is now three's FLAT_SHADED one, the cross
product of the view position's derivatives (with WGSL's `dpdy` turned to
GLSL's `dFdy`). Sierra's, Coast's and Desert's stations are unchanged by it.

Gate, the web build at 1280 × 800 through the registered server: **all 43
Streets stations within the limits**, median 0.22 / 0.51, worst 0.489 mean
and 1.812 block 95 % (02500-chase). The material scenes for StreetFacade,
StreetAtlas, AmbientProp and Neon pass (0.108 / 0.312 at worst) and join
`all`; Steam's (points) is not in the material tool's set the client draws.

## WP 7.4 Seaside decisions

## D590. The shape of `mp_worldgen::raceway`; `world.level.data` and `world.onCountdown`

2026-10-04, WP 7.4. `Raceway.js` is `mp_worldgen::raceway`
(`raceway/mod.rs`), `raceway/textures.js` is `raceway::textures`.
`Raceway` keeps what `plan()` decides (`this.corners`, from `findCorners`)
and, after `build()`, the JS fields other code reads (`kerbs`,
`tyre_runs`, `bridges`, `tree_count`, and `lamp_mats`, which the e2e
`circuit` test reads as `lampMats`); `build()` makes a `Bld` that is the
JS `this` (the track, the survey, the graph, the group `raceway`, the
rendered-surface sampler `S`, whose height cache all the builders share in
the JS order, the materials) and runs the JS `build*` methods in the JS
order; the `await tick()`s are not needed in one job. The extrusion
profiles' closures hold the track by an `Arc` clone (`road::Lat` closures
are `'static`). Where the JS `continue`s past a loop update that draws
(the crowd's seats, the near trees' cells) the port uses a labelled block,
as D530 does. The textures are `TextureCache` entries under
`raceway:kerb`, `raceway:tyre`, `raceway:fence`, `raceway:crowd` and
`raceway:banners` (D352); `bannerAtlas().rect(name)` depends only on the
panel's place in `BANNERS`, so `banner_rect` computes it without the
picture. The helpers are Mountain's `SurfaceSampler` (`coast/kit.js` is
Mountain's kit, D530), City's `GeoBuilder` and `staticMesh`, Road's
`extrude` and `runs`, Valley's `canopyGeometry`; `blobGeometry` keys its
vertices by `toFixed(3)` as Rust's `{:.3}` of `x + 0.0` (no coordinate of
the icosahedron is a rounding tie; -0 prints as 0 in both).

The JS reads two things the world did not have. `world.level.data`, the
survey, is `World::level_data` (`Option<Arc<SeasideData>>`, set with
`World::with_level_data`), which the caller sets with the level it
prepared (D54); Raceway's `build()` without it fails ("prepare() first"),
which the job logs, as the JS level's `path()` throws. `world.onCountdown`,
the hook the race calls every frame (`Race.update`:
`this.world.onCountdown?.(started ? -1 : this.countdown)`), is
`World::on_countdown` (`CountdownFn`: `FnMut(cd, &mut Vec<Edit>)`), which
`finish()` carries to `WorldBuild::on_countdown`;
`WorldBuild::countdown(cd)` runs it and resolves its edits as
`WorldBuild::update` does. Raceway's sets the five start-light materials'
`emissiveIntensity` (6 lit, 0 dark; `lit = cd > 0 ? clamp(ceil((4 - cd) ×
5 / 4), 0, 5) : 0`). Both are new fields with `None` defaults: no other
level's build changes (the Sierra, Coast and Desert gates pass unchanged).
Raceway is the last line of `scenery::PORTED`; its `plan()` registers
nothing, so the terrain and road gates are unchanged, and Seaside Raceway
now builds with `scenery_factory(None)`, from ported modules alone, with no
recorded stand-in. The shared test helper `common::world(id)` gives
Seaside's world its survey, and `tests/world_data.rs` builds through it,
so Raceway's build there has its data (the runout and carriageway it
checks are unchanged).

## D591. The L3 gate for Seaside Raceway, and what it found

2026-10-04, WP 7.4. `tools/parity/raceway-golden.mjs` (desert-golden.mjs's
pattern, in a file of its own) writes `parity/golden/seaside/seaside.json`
from the cached export: the group `raceway` as Desert's golden holds
`desert` (a line per node, the material table in canonical form, every
texture's 8×8 block means), every texture that is not a canvas (index,
source, size, channels, SHA-256), the night parameters, the night factor,
camera and lights. `tools/parity/raceway-textures.mjs`
(coast-textures.mjs's pattern) captures the group's canvases in the game
with the bundled fonts, every one held to it, lettered or not, to
`parity/golden/seaside/textures.json` (with premultiplied block means too,
D592) and the RGBA to `parity/cache/<key>/seaside/`. `tests/seaside.rs`
builds the level through `level_jobs` with `scenery_factory(None)`,
updates the sky at the export's focus, applies the night parameters and
runs the updaters once as the frozen export ran them (the menu: no race,
so `onCountdown` is never called and the lights stay dark), then holds the
group to the golden (`raceway_group`) and the scene to the export
(`seaside_is_the_export`: night parameters, counts and kinds always, the
non-canvas textures byte for byte, and with the cache the whole digest
entry by entry). Result: **identical**, native and in wasm, with no fix to
any shared module: the group `raceway`, 165 nodes (63,253 vertices) and
all 18 materials; the scene, 227 nodes, 126 meshes, 23 materials, 13
textures, 90 instance sets, 2 lights, 220 drawables, 261,352 vertices,
1,306,614 indices (the exporter's byte count, 39,061,944, comes out equal
too), every digest entry equal but the pixels of the 9 canvas textures;
the night parameters equal. The terrain's data texture and the
loose-ground mask made from the survey are byte-identical to the export;
the photo is the caller's (D593). Canvas textures against the capture:
the concrete 0.03 levels mean absolute difference at most, the kerb 0.36,
the tyre wall 0.21, the crowd 0.50, the banner atlas 0.20 (22.0 against
the export, all in the glyphs: the export draws with the machine's fonts),
the catch fence as D592 says. Rerun: `node tools/parity/raceway-golden.mjs`
(with the cache), `node tools/parity/raceway-textures.mjs` when the fonts
change, then `cargo test -p mp_worldgen --test seaside` (and in wasm).

## D592. The catch fence is held where three draws it

2026-10-04, WP 7.4. `fenceTexture` is D534's case again: diagonal strokes
1.6 px wide on a transparent canvas, where Chrome's coverage across a
stroke is 16, 137, 242, 137, 16 and mp_canvas's exact area 2, 151, 242,
151, 2 (the same total). Unpremultiplied that is 11.8/11.7/9.1/5.1 levels
mean absolute difference (R/G/B/A), the RGB of the faint edge pixels
dominating. The fence material is `alphaTest` 0.35 and opaque, so three
draws only the texels at or above the cut: no pixel falls on the other
side of it (0 of 16,384), the colour of the drawn pixels is within
0.88/0.56/1.19 levels, the premultiplied colour within 3.7/3.9/3.8 and the
total alpha within 0.12 %. `tests/seaside.rs` holds an alpha-tested canvas
over the threshold that way (no crossing, drawn colour under 3,
premultiplied under D534's 6, alpha total within 0.5 %), and without the
capture's RGBA (CI, wasm) by its premultiplied 8×8 block means, which the
capture records (0.14/0.29/0.20/0.08). Matching Chrome's stroke coverage
stays mp_canvas's (D534).

**Resolved 2026-10-04 (D650).** The fence's strokes are lone line
segments, which Chrome draws as quads with an analytic edge ramp
`|nx| + |ny|` wide (16, 137, 242, 137, 16 is that ramp at 45°, not
multisampling). mp_canvas now draws them so, and the fence is within
0.148/0.117/0.088/0.029 levels of the capture, every alpha equal;
`tests/seaside.rs` holds it to 3 like the other canvases, and the
alpha-tested criterion (`alpha_tested`, `STROKE_PREMULTIPLIED`, the
premultiplied block means without the RGBA) is gone. The golden still
records the premultiplied block means; nothing reads them.

## D593. The photo in the gate; the loose-ground mask byte for byte

2026-10-04, WP 7.4. The draped photo and its mask were ported with the
terrain (D234): the client decodes `photo.jpg` and passes it in
`TerrainSetup::photo` (`GroundPhoto::seaside`); the mask is made from the
survey. With the cache, `tests/seaside.rs` takes Chrome's decode from the
export, as `tests/terrain_mesh.rs` does, so the whole digest (the photo's
bytes and sampler, the terrain material's `tPhoto`, `tLoose` and
`uPhotoBox`) is compared; without it a blank picture of the photo's size
(1843×2160) stands in, and the photo's bytes alone go unchecked. The mask
(1106×1296, one channel) and the terrain's data texture are compared to
the golden's SHA-256 in every run, CI and wasm included: byte-identical.

## D594. Seaside Raceway's start lights and animators, step by step

2026-10-04, WP 7.4. Raceway registers no updater; its moving part is
`world.onCountdown`. `tools/parity/seaside-animators.mjs`
(desert-animators.mjs's pattern, D555, in a file and golden of its own,
so that other packages can extend `animators.mjs`) runs the game's own
`world.update` and then `world.onCountdown(cd)` over 16 uneven frames
(dt 0 to 3.3 s round the lap and past the line, the camera at the line,
the hairpin and the Corkscrew, the countdown lighting each column, GO,
racing at -1, a fresh start) and 360 ticks of 1/120 s at 60 m/s with the
countdown running from 4 to 0 (`cd = 4 - (k + 1) / 90`), recording every
value that changes under `world.root`: 5 values on 5 targets, the
start-light materials' `emissiveIntensity` (the road's dew and the night
parameters hold still: Seaside's sky is at night 0 all race). The Node
capture (the photo an empty texture, which nothing reads; three's
"already non-indexed" warning from `blobGeometry`, which the game gives
too, let through) and two browser captures agree; CI checks the Node one
reproduces. `tests/seaside_animators.rs` replays them as the client runs a
frame (`update_sky`, the night parameters, `update`, then
`WorldBuild::countdown(cd)`): **identical, every frame and every tick**,
native and in wasm.

## D595. What the client still stands in for on Seaside Raceway

2026-10-04, WP 7.4. The data side of every material kind Seaside Raceway
uses is complete (D591): Terrain (with the photo), Asphalt, Markings,
SkyDome and 19 built-in Standard materials. On `main` today none of them
is stood in for or hidden by `convert::stand_in` (no Sprite, no effect
`ShaderMaterial`): the Terrain patch already draws the drape (`MR_PHOTO`:
`tPhoto`, `tLoose`, `uPhotoBox`), and Raceway's materials are plain
`MeshStandardMaterial`s whose features the renderer has (maps, vertex
colours, `instanceColor` on the oaks, `alphaTest` on the fence,
`polygonOffset` on the kerbs, the grid and the pit lane, `DoubleSide`).
What the client still has to do for the L4 stations and the race:

- **Build Seaside from mp_worldgen** (`animate::generated` lists Sierra
  only, D491; Seaside builds whole now, numbered as its export): the level
  prepared with the survey (`seaside::prepare`), then
  `World::new(level).with_level_data(survey)` with the same
  `Arc<SeasideData>`; `TerrainSetup { ground_color:
  Some(seaside_ground_color(survey)), photo: Some(GroundPhoto::seaside(
  &survey, decoded_photo_jpg, url)), .. }` (D234: the decode is the
  client's; the JS sets the texture sRGB, `flipY` false, anisotropy 8,
  clamped; for the animators alone, which never read it, a blank picture
  of its size does, as `tests/seaside_animators.rs` builds);
  `scenery_factory(None)`. Without the survey Raceway's build fails and
  the job logs it (D590).
- **The start lights** (`Raceway.js` `buildStart`, `world.onCountdown`):
  five `MeshStandardMaterial`s (colour 0x220806, emissive 0xff2010,
  `emissiveIntensity` 0), two `CircleGeometry(0.17, 12)` lamps each, the
  material's `emissiveIntensity` changed per frame. Every frame of a race,
  after `world.update`, the client calls `WorldBuild::countdown(cd)` (`cd`
  the countdown's seconds left while it runs, -1 once racing: `Race.js`
  `this.world.onCountdown?.(started ? -1 : this.countdown)`) and applies
  its `Change::Number { prop: "emissiveIntensity" }` edits to the
  material's emissive intensity (6 lit, 0 dark) as D493 applies
  `WorldBuild::update`'s (D490's material block). The e2e `circuit` test
  counts the lit lamps (`__world.scenery[0].lampMats`).
- The lap HUD and the rest of the race (not world generation).

## D458. The race's car models join the warm-up

2026-10-04. Every race compiled one (Coast) to three (Sierra) pipelines
after `ready` (`__mp.lateFrames`): the car models (D440) are built when the
race starts, during the warm-up, but most of their parts are hidden then
(the traffic not yet on the road, the far models, the near models of cars
drawn far), and Bevy specialises a pipeline only for what is drawn.
`play::models::spawn` now notes each part's material key, mesh layout and
shadow casting in a `warmup::Combos`, as the loader does (D390), and
spawns its stand-ins: every kind and material variant of the field,
racers and traffic, near and far models, lights included. The police are
not drawn in a race yet (the race draws players, rivals and traffic,
`flow::slots`), so there is nothing of theirs to warm up; when M8 draws
them, their models go through the same spawn. A restart builds a new field
after `ready`; its stand-ins are drawn for a frame and removed, and its
combinations are the first race's. In the race runs since (Coast and
Sierra, both backends, 30 and 10 s from the start, 20 runs) `lateFrames`
is 0.

## D459. The HUD's glyphs are laid out during the warm-up

2026-10-04. With the late pipelines gone, WebGPU races still had 50 to
130 ms frames in the first half second of racing (s 42 to 45). Counting
the WebGPU calls per frame there: nothing changed but one or two
`writeTexture`s of 512 × 512 a frame, the Bevy UI text's glyph atlas
uploaded whole each time the race timer showed a digit not yet in it;
in Chrome those uploads go through the GPU process's command stream (the
D455 signature: the page waiting for command space). `warmup::spawn_glyphs`
lays out all printable ASCII (the HUD shows nothing else) in each of the
HUD's text styles (`play::hud`: bold, sizes 84, 44, 30, 26, 22, 17 and 15),
hidden and off screen, beside the cars' stand-ins; they go when the
warm-up ends, and the atlas keeps the glyphs. The sizes are copied from
`play::hud`; a new HUD size must be added there too. After it, no atlas
upload happens at the start, and the first frames of racing take 5 to 17
ms (load 27).

## WP 5.6–5.7 and race audio decisions

## D510. The songs' L4 renders: all seven within 0.4 dB, and the songs join the default run

2026-10-04, WP 5.6. The last gate of WP 5.6 is SPEC 7.5's offline renders
of each song's first thirty seconds (`song-<id>` in `renders.json`, music
at 0.7 through the music bus, SFX silent). They render on the native
backend through `examples/render_scenarios.rs` as the other scenarios do
(D253: `music.play(track)`, `setMusic(true)`, `pumpUntil(t + 1)` every 64
control frames) and compare with Chrome's band levels in
`tools/parity/audio-bands.mjs`. Nothing needed fixing: the worst band per
song is midnight-run 0.36 dB (1259 Hz), neon-rush 0.36 dB (1259 Hz),
mirage 0.17, seabright 0.13, afterburner 0.07, interstate 0.02 and
chrome-heart 0.01 dB, against SPEC 7.5's 1.5 dB and Chrome's own run-to-run
jitter; the RMS levels agree to 0.03 dB. The sequencer's call log was
already exact (D251), so these renders check the instruments' DSP (the
hall convolver, the delays, the drum kit's buffers, the pulse waves) on the
native backend. `audio-bands.mjs` now renders and compares every scenario
by default, the songs included (111), instead of leaving the songs to an
explicit prefix.

## D511. A param event scheduled before a target curve re-anchors the curve (the burble)

2026-10-04, WP 5.7. `shot-radio-burble` was 6.4 dB off at 158 Hz (D259).
The burble schedules every syllable's `setTargetAtTime` on the buzz's
frequency first and sets `buzz.frequency.value = f0` after: a set event
at the present, before all of them. D254's workaround had already sent
each target to the crate behind a `setValueAtTime` holding the value the
param had when the target arrived, the oscillator's default 440 Hz, so
the first syllable swept down from 440 Hz where Chrome's starts at f0
(the first 0.3 s carried 15 dB more at 316 Hz). The native backend now
re-sends a param's events from the new event's time on (cancel, then the
mirrored timeline with every target anchored anew) whenever a value, set,
ramp or target lands before a target curve already sent. The burble now
matches Chrome to 0.01 dB in every band, and the full L4 run is 110 of
111 within 1.5 dB; the one left is `engine-rally-6500-1`'s 25 Hz band
(D259), 60 dB under the signal.

## D512. Radio lines in `mp_audio::radio::lines`; `CALLSIGNS` in `mp_sim::pursuit`

2026-10-04, WP 5.7. `radioLines.js` is `mp_audio::radio::lines`, per SPEC
7.3's port map: `DIRS`, `TAKES`, `place_name`, a `Line { text, parts }`
per `RADIO` entry as a function of the same name (`unit_down`,
`rival_busted`), `radio_clips` and `levels_radio_clips` in the JS's order
(a `Vec` stands in for the `Map` by id, first entry wins). The JS takes
`CALLSIGNS` from `Pursuit.js` as `radioClips`' default units; `mp_audio`
may depend on `mp_math` only (SPEC 3.2), so the units are an argument and
the callers pass `mp_sim::pursuit::CALLSIGNS`, a new export computed from
the chase pool as the JS computes it (10 to 30). `levels_radio_clips`
takes a `LevelRadio` (police or not, zone names, rival names) per level
for the same reason. The whole clip list (192 clips) was compared once
with the JS's `levelsRadioClips(LEVELS)`, id, text and takes, and is
identical. `test/unit/radio.test.js` and `pursuit-audio.test.js` are
`tests/radio.rs` and `tests/pursuit_audio.rs`; they read the levels and
the callsigns through dev-dependencies on `mp_track`, `mp_levels` and
`mp_sim` (check-deps looks at normal dependencies only), and the check of
`audio/radio/` against `index.json` runs natively, the rest in wasm too.
The fake `fetch` and decoder are a `Fetch` that serves names as bytes and
the null backend's decoder, which tells the files apart by the length it
decodes them to. The sirens, the radio bus, the burble and the pursuit
calls were already ported with `GameAudio` (D251); the stingers and the
radio are exercised by the pursuit drive's call log, which stays exact.

## D513. The race's audio: `play::audio`, one `GameAudio` shared with the gesture bridge

2026-10-04, race audio. `crates/mp_game/src/play/audio.rs` drives
`mp_audio`'s `GameAudio` the way `main.js` and `Race.update` drive the
JS one. `GameAudio` holds `Rc` handles, so it lives in a non-`Send` Bevy
resource (`Shared`, an `Rc<RefCell<RaceAudio>>`) whose system runs after
`draw` on the main thread; on the web the same `Rc` is in a thread-local
that the page's gesture handlers reach through `web::gesture`. The
backends are the facade's: the browser's Web Audio on wasm (`mp_audio`'s
`web` feature), natively web-audio-api with an output device
(`native-device`: cpal, so ALSA headers on Linux; CI installs
`libasound2-dev`). `native::try_context` returns `None` where no output
stream can be made, and the game is then silent instead of panicking.
The radio clips are fetched through the `Fetch` trait: `../../audio/radio/`
relative to the page on the web (as Seaside's survey is fetched, so it
works under any sub-path), `audio/radio/` under the repository natively;
the files are untouched. `play/mod.rs` gains `pub mod audio` and one
`audio::plugin(app)` line; `flow.rs` and `session.rs` gain the per-tick
observer (D515); `web.rs` gains one call in `gesture`.

## D514. The graph is built behind the loading screen; the first gesture starts it

2026-10-04, race audio. `GameAudio::init` builds every buffer, wave and
node of the graph: 0.19 s natively in release on the null backend, 0.48 s
on the native backend, 0.31 to 0.57 s in the browser (the WebGPU release
build in headless Chrome on the dev machine, under load). In the JS game it runs inside the tap on
Start, before the race. The Rust client has no menu yet (M6) and starts
the race on its own, so the first gesture is a touch or key during the
race; building there would stall the race for a third of a second or
more on a phone. So the client calls `setCar(car)` then `init()` once the
race is made and before it starts, behind the loading screen: the context
is created suspended (outside a gesture a browser does not start it), and
nothing plays, since a suspended context's clock stands still and
`GameAudio` does not steer before it runs. `init` is documented in the
JS to work this way ("the graph builds fine on a suspended context;
unlock() starts it from a gesture"). Every gesture then runs the JS's
`wakeAudio` (`init(); unlock()`) inside the page's handler: pointer-down,
pointer-up, touch-end, click and key-down, the JS's five (the page gains
`pointerdown`; on an iPhone the tap's pointer-up and touch-end are the
gestures, and WP 5.3's `audioSession` "playback" is set in `init` before
the context is made, so the Silent switch does not mute it). When the
race starts, the client makes `startRace`'s calls in its order:
`setPaused(false)`, `init()`, `unlock()`, `playTrack` (`pickMusic`, only
when the level or the choice changed), `setVolume`, `setMusic` and
`setCar`. Natively there is no autoplay rule and the context runs from the
start. On a phone the sound starts when a finger first lifts (a touch's
start is not a gesture to a browser): a player who holds the stick and
the pedal from the countdown on hears nothing until then. The JS game's
players tap Start first; the client gets its start button in M6.

## D515. The audio hears every tick, and the camera as the JS's audio reads it

2026-10-04, race audio. `Race.update` calls the audio once per update,
and the parity drives update once per 1/120 s tick. The client's session
now calls an observer after each tick with the state, the tick's events
and its input (`Session::advance_observed`), and `flow::Race` keeps a
`TickAudio` per tick of the frame: the one-shots from the `SimEvent`s
(countdown and GO beeps, the player's car contacts and wall impacts,
shifts, landings, near-miss and passing whooshes, the finish fanfare),
`update`'s state (with the off-road amount computed as `Race.update` does:
the road type's edge, the loose ground), the nitro, the rivals relative to
the player, the tunnel spans, the car for the camera and the camera bumps.
The audio system replays them in order after the frame. `update`'s
throttle is the input's during the countdown and `ctrl.throttle` after it;
when the controls are not the input (the cool-down driver after the
finish, the hold of a bust) the simulation now says so with a new event,
`SimEvent::Controls { player, throttle }` (state, hash and traces
unchanged).

The pans the JS takes from `camera.matrixWorld` (a contact's, the
rivals') are not the drawn camera's. `Object3D.lookAt` brings the world
matrix up to date before it sets the new orientation, so the matrix holds
the orientation of the `lookAt` before: during the countdown the rivals
hear the chase camera of that tick (`cam.update`'s `lookAt`), not the
intro swing that is drawn (`introCamera`'s), and after it the previous
tick's camera; a contact, read earlier in the update, hears the camera of
one or two ticks before, depending on whether a render came between.
With the drawn camera the countdown's pans were up to 1.85 off (the
swing goes the other way round). So the audio driver steps its own
`CameraRig` per tick on the tick's car, with the drawn rig's mode and the
frame's look-back and the same bumps, and keeps the two orientations the
JS keeps: the last `lookAt`'s and the matrix's, which takes the former at
each `lookAt` and at the end of each rendered frame. Over the scripted
race the 16,326 pans then agree with the JS's to 8e-16.

## D516. The call-log gate for the race in the client

2026-10-04, race audio. Two checks against `drive-race.jsonl.gz` (the JS
game's calls on its audio over the 5400 ticks of the scripted race:
Sierra, the sports car, seed 1, the autopilot). `play::audio::tests::
the_scripted_race_makes_the_js_calls` runs the client's frame loop
headless at 60 frames a second (two ticks a frame, as the drive) and
requires all 10,962 calls, line for line, every argument bit for bit
but the 16,326 pans, which agree to 8e-16 (three's quaternion round
trip). `node tools/parity/rust-audio-race.mjs` runs the web release build
in headless Chrome with a phone's autoplay rule, reading the page without
user activation, and `?audiolog=1` (the client records its calls as the
JS facade recorder writes them): before any gesture the graph is built
and the context stays suspended with its clock at 0 for two seconds of
countdown; a key press starts it; the race's 10,962 calls then match the
JS drive's, method, tick and every argument but the pans exactly, the
rivals' pans to 8e-16, and the car contacts' pans to 0.014 (74 to 89 of
the 126 impacts differ beyond 1e-9: a contact hears the camera of the tick before or the
one before that depending on whether a render came between, and the
browser's frames are not the drive's two ticks long). The gesture's own
`init`/`unlock` and the build behind the loading screen (`setCar`,
`init`) are reported and left out. Since the Rust `GameAudio` turns the
JS drive log into the JS Web Audio call log exactly (D251,
`tests/game_calllog.rs`), the client's Web Audio calls are the JS game's
but for those contact pans. What the audio costs the main thread is on
`window.__mp.audio`: 0.35 ms a frame on average in that run, 11 ms on the
race's first frame (`startRace`'s calls), and nothing in the measurement
page (`?perf=1` flies the camera with no race, so no audio is made).

## D517. Settings, keys and what is left to the menus and to Hot Pursuit

2026-10-04, race audio. The music and SFX volumes and the track choice
are read from the JS game's store keys (`mr.musicVol`, `mr.sfxVol`,
`mr.track` in `localStorage`, JSON), so a player who set them in the JS
game hears the same; natively the JS defaults (0.7, 0.85, the level's
own). The music key (M) toggles the music between 0 and 0.7 and stores
`mr.musicVol`, as `main.js` does; T plays the next track. Pause and resume
call `setPaused`; the results' race again calls `uiClick('start')` before
`startRace`, as the JS button does. Not done here: the volume sliders, the
track picker and the now-playing toast (M6's menus and HUD), and Hot
Pursuit's audio hook-up (sirens, mood, damage, spiked tyres, the radio
lines and their prefetch, the stingers), which is WP 8.4; the client's
pursuit races get the race's sounds only.

## WP 7.2 Streets decisions

## D610. The shape of `mp_worldgen::streets`

2026-10-04, WP 7.2. `Streets.js` is `mp_worldgen::streets` (`streets/mod.rs`),
`streets/props.js` is `streets::props`, `streets/facades.js`
`streets::facades` and `streets/textures.js` `streets::textures` (their
canvas pictures; `facadeMaterial` and `patchStreetAtlas` are in `mod.rs`
beside `ambientPatch` and `neonFlicker`, which are kinds whose GLSL is the
renderer's). The JS methods are spread over `mod.rs` (the route analysis,
the materials, the blocks and their kerb outlines, the streets, the
pavements, `emitAll`), `buildings.rs` (`block`, `midrise` and its signs,
`rowhouse`, `tower`, `plaza`, `farBlock`, the trees, the aircraft lights,
the billboards) and `dressing.rs` (the lamps, signals, barriers, gantries,
crowds, lanterns, the elevated railway, the vents, the reflections), all
`impl Bld`, where `Bld` is the JS `this` while it builds: the grid
(`level.grid` is `mp_levels::streets`: `PX`, `PZ`, `HW`, `WALK`, the
route's `setback` and `legs` from `build_route()`, `ground`, `district`),
the generator 9090, the materials, the collected lists (fronts, sign
lights, spill, steam, cables, trees, aircraft, billboards) and the ten
chunked builders. `Chunks` keeps the JS `Map` keyed `"cx,cz"`: builders in
the order first asked for, a `BTreeMap` used only for lookup. The JS
makes a chunk wherever it writes `const P = this.bPlain.at(cx, cz)`, even
when nothing goes into it, and the order of the chunks is the order of the
meshes, so the port asks for the chunk at the same place (the gate found
the one place it did not). `this.blocks` is a `Vec` in insertion order with
a lookup map; `crossUse`'s values are only tested for truth, so it is a
set. Where the JS draws from the generator in a loop condition (`k < (tier
=== 0 ? 1 + Math.floor(rng() * 3) : 0)`, the wires, the newspaper boxes)
the port draws the bound anew at every test (D350); where it draws inside
an argument list or an array literal, into locals in the same order.
`Props.parkedCars` bakes `car_model::build_vehicle` (`lod: 'low'`, seed 3)
as `bakeCar` does, through `mountain::kit::world_matrices`;
`setHeadlights(0)` changes only emissive intensities, which the bake does
not read, so it is not called. `plan()` sets the runout by
`mp_levels::world::plan_runout` (0), mirrors it into `sim_data`, and
assigns `world.no_marks` (`t.noMarks = nm`; Streets is the level's only
module). Streets is the eighth line of `scenery::PORTED`: the terrain and
road gates run its own `plan()`, which gives the recorded unpainted
stretches bit for bit, and Level 3 builds from ported modules alone. Two
JS quirks are kept, not deviations: no traffic-signal mast stands on a
kerb (the corner points fall outside the kerb's rounded corner), so
`buildSignals` builds nothing but still draws its coin flips; and the
billboards' posts go into a plain builder after `emitAll` has emitted it,
so nothing draws them.

## D611. The steam puffs' `Math.random`

2026-10-04, WP 7.2. `buildSteam` gives each puff `aSeed = (k / 10,
Math.random())`, from the page's `Math.random`, which the scene export
seeds (D20). The 900 seeds of the export are 900 consecutive draws of
mulberry32(0x5eed) from draw 15,284 on (found by scanning the stream for
the export's first three values; all 900 then match as f32), so
`streets::props::STEAM_RANDOM_AT` is 15,284 and the port draws them from
`valley::page_random(STEAM_RANDOM_AT)`, as D332 does Valley's planks. A
level built for play draws the same seeds. Nothing else in Streets reads
`Math.random`.

## D612. Downtown Streets' animators, step by step

2026-10-04, WP 7.2. The updaters are, in the JS order, `NeonClock` (the
one `uNTime` uniform the two neon materials share; the port writes it into
both), `Signals` (none on Streets, D610), `Blink` (the barrier flashers'
colour), `Flashes` (the phone flashes' point size), `Train` (the elevated
train's position: scripted to meet the player from 380 m before the
crossing to 160 m after it, free-running at 16 m/s and wrapping
elsewhere; it reads the level's ground, so it keeps nothing of the
build), `Steam` (its clock, and `uScale` = 0.5 × the drawing buffer's
height (or 720) / tan(fov / 2) from the camera) and `Aircraft` (the warning
lights' colour). `tools/parity/animators.mjs --level streets` captures the
game's `world.update` over 16 uneven frames (dt 0 to 150 s, the player
under the viaduct and away from it, one long frame letting the train wrap)
and 360 ticks under the viaduct, as D537 does Level 2's: 7 values on 7
targets, in `parity/golden/animators/streets.json`. The Node capture and
two browser captures agree; the Sierra and Coast goldens are unchanged;
CI checks it regenerates. `tests/streets_animators.rs`: **identical,
every frame and every tick, native and in wasm**.

## D613. The L3 gate for Downtown Streets, and what it found

2026-10-04, WP 7.2. `tools/parity/streets-golden.mjs` (coast-golden.mjs's
method, D531, in a file of its own) digests the groups `streets` and
`road` (whose `asphalt2` Streets wets: roughness 0.62, metalness 0.05,
colour 0.72) of the cached export into `parity/golden/streets/streets.json`.
`tools/parity/streets-textures.mjs` (coast-textures.mjs's method, D535)
captures every canvas of both groups as Chrome draws it with the bundled
fonts: 22 pictures for 31 texture entries, in
`parity/golden/streets/textures.json` and `parity/cache/<key>/streets/`.
`tests/streets.rs` builds Level 3 with `scenery_factory(None)`, updates the
sky at the export's focus, applies the night parameters, runs every
updater once (dt 0, s 0, the export's camera) and holds (a) both groups to
the golden and every canvas to the capture within WP 3.2's threshold, and
(b) the whole scene to the export as Level 2 is (D536). Result:
**identical**, native and in wasm: the group `streets` 346 nodes
(2,379,330 vertices) and its 40 materials (StreetFacade, StreetAtlas,
AmbientProp ×5 with their `rgb`, Neon ×2, the Steam `ShaderMaterial` with
its GLSL, the built-ins) parameter by parameter; the group `road` 14 nodes
and 3 materials; the scene 441 nodes, 432 meshes, 45 materials, 26
textures, 10 instance sets, 434 drawables, 2,706,726 vertices, 1,855,224
indices, every digest entry equal but the pixels of the 24 canvas
textures. Against the capture the pictures are within 1.25 levels mean
absolute difference: the neon atlases 1.23/1.25/1.18 and 1.04/0.96/0.94
(R/G/B; the shadow-blurred glyphs, as Desert's neon, D551), the ads 0.19
to 0.82, the banners 0.17 and 0.06, the shared glow 0.58, the barrier 0.42,
the puddle (`filter: blur(6px)`) 0.23, the façade, street and train
atlases 0.02 to 0.10, the pavement 0.06. The lettered ones differ from the
export by up to 36 levels (the start banner), all in the glyphs (the
export's machine fonts).
The one port fix the gate found was a chunk's creation order (D610). No
shared module's behaviour changed; mp_canvas gained `stroke_text_max`
(`strokeText(text, x, y, maxWidth)`, beside `fill_text_max`, for the neon
signs), an export only. Rerun: `node tools/parity/streets-golden.mjs`
(with the cache), `node tools/parity/streets-textures.mjs` when the fonts
change, then `cargo test -p mp_worldgen --test streets` (and in wasm).

## D614. What the client still stands in for on Downtown Streets

2026-10-04, WP 7.2. The data side of every material kind Downtown Streets
uses is complete (D613). `convert::stand_in` and `render::material` on
`main` today draw these with stand-ins or hide them; the list for the L4
stations:

- **StreetFacade** (`streets/facades.js` `facadeMaterial`; the upper
  floors, far blocks, towers and roofs, one mesh per 560 m chunk): drawn as
  a plain lit standard material, its atlas sampled at the raw tile-unit
  uv. A `MeshStandardMaterial` (map and emissiveMap the 1024² façade atlas,
  4×4 cells of 256 px, clamped; emissive white, emissiveIntensity 1.1,
  roughness 0.62, metalness 0.2; no vertex colours), program key
  `streets-facade`, attributes `position`, `normal`, `uv` (tile units,
  unbounded), `cell` (atlas cell + 16 × the building's seed) and `fdata`
  (street-level y, bounce strength, bounce hue; GeoBuilder's `color`
  renamed). The patch: `vCell`, `vAUv`, `vFData` and the world y `vWY`
  from the vertex stage; replacing `map_fragment`, `fIdx = mod(floor(vCell
  + 0.5), 16)`, `fSeed = floor((vCell + 0.5) / 16)`, `fUv = ((cx +
  clamp(fract(u), 0.004, 0.996)) / 4, (3 − cy + clamp(fract(v), 0.004,
  0.996)) / 4)`, sampled with `textureGrad` and `dFdx/dFdy(vAUv) × 0.25`;
  replacing `emissivemap_fragment`, `totalEmissiveRadiance *=
  emissive(fUv) × windowLight(floor(vAUv × FSPEC[fIdx].xy), fSeed,
  FSPEC[fIdx].z)` (`FSPEC` is `GRID`: columns, rows, lighting kind;
  `fHash`, `fHue` and `windowLight` are `facades.js:363–390`), then the
  street bounce `+= diffuse × (bc × fdata.y × exp(−max(vWY − fdata.x, 0) /
  7) + (0.03, 0.032, 0.045))`, `bc` the sodium `(1, 0.68, 0.4)` or, for a
  hue `fdata.z > 0.001`, `mix((1, 0.7, 0.45), fHue(fdata.z), 0.65)`.
- **StreetAtlas** (`streets/textures.js` `patchStreetAtlas`; shop fronts,
  rowhouses, awnings, vending machines, stalls, posters): drawn as a plain
  lit standard material. A `MeshStandardMaterial` (map and emissiveMap the
  1536×1152 street atlas, 4×3 cells of 384 px, clamped; emissive white,
  emissiveIntensity 1.2, roughness 0.7, metalness 0.05, `vertexColors`
  for the tint), program key `street-atlas-2`, attributes `position`,
  `normal`, `uv` (tile units; a rowhouse's are mirrored, so negative),
  `color`, `cell`. The patch as the façade's but `sUv = ((cx + clamp(fract
  u)) / 4, (2 − cy + clamp(fract v)) / 3)` with gradients × (1/4, 1/3);
  the emissive × `wl`, `wl = 0.65 + 0.6 × fHash(vec2(sSeed, 5))` for a
  cell of kind 0, else `windowLight(floor(vAUv × SSPEC.xy), sSeed,
  SSPEC.z) × 1.4` (`S_GRID`); then `totalEmissiveRadiance += diffuse ×
  (0.075, 0.06, 0.05)`.
- **AmbientProp** (`streets/props.js` `ambientPatch`; program keys
  `streets-amb-pave`, `-plain`, `-car`, `-tree`, `-steel`; the pavements,
  the plain dressing, the parked cars, the trees (an InstancedMesh with
  `instanceColor`, `flatShading`) and the viaduct's steel and braces (an
  InstancedMesh)): drawn as plain lit standard materials, so unlit sides
  go black at night. The patch adds, after `emissivemap_fragment`,
  `totalEmissiveRadiance += diffuseColor.rgb × kind_opts.rgb`.
- **Neon** (`streets/props.js` `neonFlicker`; program keys `streets-neon-h`
  and `-v`; the fascia and blade signs): drawn as a plain unlit basic
  material, steady. A `MeshBasicMaterial` (map the 1024×512 horizontal or
  vertical neon atlas, colour (2.4, 2.4, 2.4), HDR, not clamped), uniform
  `uNTime` (the `NeonClock` animator's `Number` edit on both materials every
  frame), attributes `position`, `normal`, `uv`, `ndata` (seed, mode, 0;
  GeoBuilder's `color` renamed). The patch, before `opaque_fragment`: `sd
  = ndata.x × 97`, `k = 0.94 + 0.06 sin(t·60 + sd)`; mode 1 `k ×=
  nHash(floor(t·13) + sd) < 0.18 ? 0.12 : 1`; mode 2 `k ×= fract(t·0.35 +
  ndata.x) < 0.72 ? 1 : 0.06`; mode 3 `k ×= 0.35 + 0.65 step(0.45,
  nHash(floor(t·7) + sd))`; `outgoingLight ×= k`, with `nHash(p) =
  fract(sin(p × 91.345) × 47453.5453)`.
- **Steam** (`streets/props.js` `buildSteam`; one `Points` of 900 puffs,
  `userData.dynamic`): hidden (`stand_in`'s `Steam => Hidden`). A
  `ShaderMaterial` with its GLSL in the scene (`gl_PointSize = (0.8 +
  life × 3.2) × uScale / −mv.z`, `gl_PointCoord` in the fragment stage,
  premultiplied output), transparent, `depthWrite` false, `CustomBlending`
  One / OneMinusSrcAlpha, no fog; uniforms `uTime` and `uScale`, both
  `Number` edits of the `Steam` animator every frame (`uScale` from the
  camera: 0.5 × drawing-buffer height / tan(fov / 2)); attributes
  `position` and `aSeed` (vec2). Points become quads (D294).
- Built-ins to mind: the lantern globes (671 instances, D452) are a
  `MeshBasicMaterial` of colour (2.6, 0.5, 0.25) times `instanceColor` up
  to 2.8, so up to 7.3 in red: the data equals the export bit for bit; the
  renderer must multiply and not clamp. The lamp lenses are a basic colour
  (4, 2.9, 1.7). The aircraft lights are a `PointsMaterial` with
  `sizeAttenuation` false and `frustumCulled` false, their colour a
  `Color` edit; the barrier flashers' colour and the phone flashes' size
  are edits too. The elevated train is a `Mesh` the `Train` animator moves
  (`Transform`; `matrixAutoUpdate` stays on). Puddles, spill, reflections
  and lamp pools are additive basic materials with polygon offset −4/−4
  and render order 2; the lane paint −2/−2, the side-street asphalt +1/+2.
  The billboards are double-sided basic materials (colour 1.5) on the ad
  textures.

## WP 7.5 Cruise decisions

## D630. The loop was ported with City; WP 7.5's world-generation half is its gates

2026-10-04, WP 7.5. The roadmap row names the loop variants in `City.js`
and `city/freeway.js` and the chunk cut-off. Both were ported in WP 3.8
with the rest of City (D350): the districts, warehouses, container yards,
the waterfront and ferris wheel, the loop sites and tunnel names, the
sound-wall spans, the 2000 m building and ground chunks, the 1800 m
freeway chunks, `emitInstanced`'s spatial chunks and `fadeable`, the
cut-off that hides a mesh when the camera is further than its distance
from its bounding sphere (2600 m the ground chunks and the street-lamp
lenses, 2200 their light pools, 1900 the poles and arms, 1700 the trees,
1300 the parked cars); and D354 already held the group `city` of the
loop to the export, node by node. City is the loop's
only scenery module, so the Night City Cruise builds from ported code
alone (`scenery_factory(None)`), and no Rust source changed in this
package: what it adds is the loop's gates, in the pattern of the other
levels.

- **Per group**, as before: `city` in `tests/city.rs` (194 nodes,
  1,901,864 vertices, 43 materials parameter by parameter, every texture
  within WP 3.2's threshold, the seven lettered canvases against
  `city-textures.mjs`'s capture with the bundled fonts, at most 0.55
  levels; the capture reproduces, `--check`), the road and the sky in
  `tests/road.rs`, the ground in `tests/terrain.rs` and
  `tests/terrain_mesh.rs`, the world data in `tests/city.rs` and
  `mp_levels`' `tests/world_data.rs`.
- **Whole level**, new: `tests/cruise.rs` holds the scene as Level 1 and
  2 are held (D472, D536): built, the sky at the export's focus, every
  updater once at the export's frame (dt 0, night 1, the export's camera
  and drawing buffer), the edits applied to the objects. The counts and
  kinds equal the world golden always (CI and wasm); with the cache the
  whole `mp_scene` digest equals the export's entry by entry, and, since
  the digest leaves them out, so do every node's visibility (the 52 nodes
  the cut-off hid at the export's camera) and the night parameters.
  Result: **identical**, native and in wasm: 495 nodes, 407 meshes, 49
  materials, 19 textures, 94 instance sets, 2 lights, 486 drawables,
  2,844,902 vertices, 4,989,900 indices (even the exporter's byte count,
  180,093,320, comes out equal), but for the pixels of the 17 canvas
  textures that differ, which the threshold gates hold (City's in
  `tests/city.rs`, the shared terrain and road pictures in
  `tests/textures.rs`). So the data side of every material kind the loop
  uses is complete: Terrain, Asphalt, Shoulder, Markings, SkyDome,
  CityFacade, GlowPoints, TrafficStreams, SkyGlow and the built-in
  Standard, Basic and Points.

Rerun: `cargo test -p mp_worldgen --test cruise --test city` (and in
wasm); `node tools/parity/city-textures.mjs` when the fonts change.

## D631. The Night City Cruise's animators, step by step

2026-10-04, WP 7.5. Sierra's capture (D470) never runs the cut-off, which
only the loop has. `tools/parity/animators.mjs --level cruise` builds the
loop with the game's own `World.build` and runs its `world.update` over 16
uneven frames (dt 0 to 150 s, s from 0 to the loop's length) and
360 ticks of 1/120 s at 60 m/s from 300 m before the first downtown, with
the camera hopping between three places a third of the loop apart (the
start, the first downtown's middle, the second viaduct's middle; every
40 ticks in the run), so that what one place sees the others cut off. It
records 86 values on 86 targets in `parity/golden/animators/cruise.json`:
the visibility of 82 of the 101 meshes and instanced chunks on the fade
list (the other 19 are seen, or cut off, from all three places), the
ferris wheel's rotor (a `Group`'s quaternion), the freeway's chase bulbs
(62 instance colours of one InstancedMesh), the aircraft warning lights'
blink (a `PointsMaterial` colour) and the traffic streams' `uTime`. The
loop's sky is pinned at midnight (night 1), so the night's values (lamp
lenses and pools, the neon, the glow points, the sky glow's `uK`, the
road's dew) are written every frame and hold still; the test requires
that too. The Node capture and two browser captures (`--browser`) are
byte-identical; CI checks that the Node capture reproduces; the Sierra,
Coast and Streets goldens are unchanged. `tests/cruise_animators.rs`
replays the frames on the Rust build as the client runs them (D470's
method): **identical, every frame and every tick, native and in wasm**;
no port fix was needed.

## D632. What the client still needs for the Night City Cruise

2026-10-04, WP 7.5. Every material kind the loop uses is drawn by the
client since D494 (CityFacade, TrafficStreams, SkyGlow, GlowPoints, the
terrain and road kinds); `convert::stand_in` hides none of them and
stands in for none. What is left is how the level is loaded and
animated, on `main` today:

- **The world build.** `animate::generated` is true for Sierra only, so
  the loop gets no world build and no animators: nothing is cut off, the
  chase bulbs, the aircraft lights and the traffic streams stand still,
  the ferris wheel does not turn. The loop's build is numbered as its
  export (D630), so `generated` can return true for `cruise`, with
  `?world=gen` following.
- **Nodes exported hidden.** The loader does not spawn a node exported
  invisible (D493's "not covered"). The export was taken with the cut-off
  run at its camera, so 52 of the loop's nodes (chunks of the ground and
  of the street lamps' poles, arms, lenses and pools, the trees and the
  parked cars) are exported hidden and are never drawn, even
  when the camera drives up to them, and the cut-off's `Visible(true)`
  has nothing to show. They need to be spawned hidden (or at least those
  an animator addresses), so that a `Visible` edit can show them. With
  `?world=gen` the build's scene has them all visible until the first
  update, so this bites the default (download) path only.
- **Per-frame edits.** 178 a frame: 101 `Visible` (the whole fade list
  every frame, as the JS sets `visible` every frame; only changes need
  applying, which D493 already does; the cut-off needs the camera's
  position, and does nothing on a frame without a camera), 62
  `InstanceColor` (the chase bulbs, one InstancedMesh), one `Transform`
  (the ferris wheel's rotor, a `Group` whose subtree turns), and material
  values: colours of the freeway's lamp lenses (`MeshBasicMaterial`,
  4.5) and light pools, the neon, City's street-lamp lenses and pools
  (each one material shared by 16 instanced chunks), the aircraft lights
  (`PointsMaterial`, `sizeAttenuation` false) and the glow points; the
  numbers `uTime`, `uNight`, `uFogK`, `uHalfH` (TrafficStreams), `uK`
  (SkyGlow), `uFogK` (GlowPoints) and `uWet` (Asphalt). Ten materials
  get animation blocks (D490).
- **Size.** The loop is the second largest scene after Coast: 2.84 M
  vertices against Sierra's 1.53 M, a 181 MB export against Sierra's
  138 MB. D491's
  memory numbers (the build and the downloaded scene not held at once)
  should be measured again on the loop against SPEC 6.6's 512 MB, and
  D492's `?world=gen` (no download) weighed for it as for Sierra.
- **The loop itself.** The sky is pinned (the JS samples it at p = 0.5
  on a loop, and so does `update_sky`), so the night factor is 1
  throughout. The cruise scoring HUD (score, multiplier and its bar,
  distance, best; the simulation's side is `mp_sim::race`'s cruise
  fields, WP 1.5) is the client's, roadmap WP 7.5's third item.

## Canvas anti-aliasing decisions

## D650. A lone stroked line is an analytic quad

2026-10-04. Measured in Chrome 151 on the reference machine (headless,
`--use-angle=vulkan`, the RTX 3060; probe pages drawn through the e2e
harness, read with `getImageData`): a stroke of a path that is one line
segment (`moveTo`, `lineTo`), with a butt or square cap, under a
transform that keeps right angles and at least a device pixel wide, is
Skia's `drawStrokedLine`: a quad (the segment's ends moved half the
width either side, and half the width along it for a square cap) with
per-edge anti-aliasing. Its coverage is not the exact area (D151): each
edge ramps linearly from 0 to 1 over `|nx| + |ny|` (the pixel's width
across the edge, 1 for an axis-aligned edge, √2 at 45°), centred on the
edge. Fitted at 3°, 10°, 20°, 30°, 37°, 45°, 60° and 80° on a 6 px line,
the ramp is within 0.0034 everywhere, the exact area up to 0.125 off.
Opposite edges combine as `c₁ + c₂ − 1` (a 1 px line at 45° is 90, 180,
90, as Chrome), and at the ends the pixel takes the smaller of the sides'
and the ends' coverage (within 0.003 at the corners of butt and square
caps). Skia draws this as an outer quad (each edge moved out by half its
ramp) at coverage 0 and an inner one at 1, and the GPU snaps both quads'
corners to its 1/256 px grid before it interpolates: mp_canvas does the
same (`raster::line_quad_coverage`), which is what moves the catch
fence's faint pixels' blue from 12.48 to 13 as Chrome rounds it. A round
cap, a thinner line, a skewing transform and any other path go to the
tessellated stroke (D652). This is the catch fence (D592): every stroke
in it is a lone line.

## D651. Multisampled strokes blend into samples until the canvas is read

2026-10-04. Chrome multisamples every stroke that is not D650's quad,
with the standard 8× pattern of Direct3D and Vulkan: a near-horizontal
edge swept over 256 columns steps at sample rows (2k + 1)/16, a
near-vertical one at the same columns, and a 45° sweep pins the pairing
to (9,5), (7,11), (13,9), (5,3), (3,13), (1,7), (11,15), (15,1) in 1/16 px
(`raster::MSAA8_X`, sorted by row). And it blends per sample: two opaque
curves of different colours crossing at a shallow angle leave only whole
eighths in alpha and in each colour, where blending their coverages would
not. So once a canvas strokes a multisampled path, mp_canvas keeps eight
samples for each pixel whose samples differ (`samples.rs`; the others
stay in the pixmap), blends every later draw into each sample (analytic
draws with their coverage, into all eight), and keeps the pixmap at the
samples' mean, a tie rounded down (Chrome's half-covered opaque pixel is
127; `(sum + 3) >> 3` matches every level seen). Reading the canvas
(`getImageData`, the source of `drawImage`) resolves the pass: the next
draw starts from the pixels, as Skia reloads the multisample buffer from
the resolved texture; `putImageData` writes after a flush and ends it
too. A stroke with a shadow takes its shadow from the outline's exact
coverage as before; a stroke under a `filter` keeps the old exact-area
layer (the game filters only fills).

## D652. Strokes are tessellated as Skia's GPU stroker does

2026-10-04. Multisampling the exact stroke outline (tiny-skia's stroker,
flattened finely) left the leaf at 0.23 alpha and single curves one to
three samples off along their length: Chrome's GPU follows a curve with
straight pieces a quarter pixel from it. `tess.rs` ports
`StrokeTessellator` and `GrStrokeTessellationShader` (Skia main,
2026-10): each segment is a patch (a line as `p0, p0, p1, p1`, a
quadratic as its cubic, a curve needing more than 32 parametric segments
chopped evenly first); its edges are the union of Wang's formula's
parametric segments (precision 4) and radial segments (`0.5 / acos(1 −
1/(4r))` per radian of turn, r the device radius), each edge placed by
the shader's own search, at the curve's point plus and minus the radius
along its normal; consecutive edges make the strip's two triangles. Joins
are fans of edges around the junction on the outer side (miter: the
outer corners and the miter point, `miter_extent` falling back to the
bevel past the limit; bevel; round: radial segments), none where the
tangents are nearly parallel. Butt caps add nothing, square caps a line
of half the width at each end, round caps a stroke-width circle of
radial edges. The triangles are mapped to device space, snapped to the
1/256 px grid and rasterised exactly in integers with the top-left rule
(`raster::msaa_triangles`); a sample in any triangle is covered once.
Single precision as the shader, transcendental functions through the
kernel, so native and wasm agree. Against Chrome, sample for sample:
quadratic strokes 1.6, 3 and 10 px wide, a three-point polyline, cracks
0.8 and 1.2 px wide (the rock's and the asphalt's), a `rect()` path and a closed
polyline with round joins and caps are identical; a native `roundRect`
and an arc with lines differ in 80 pixels of 65,536 by a sample or two,
a full circle in 353 (0.22 levels): Blink makes arcs conics where
mp_canvas makes cubics (D150's path code), and a lone `arc()` is Skia's
arc op. Not chased: no texture of the game strokes a lone circle.

## D653. After five path draws, lone lines are multisampled too

2026-10-04. The corn strip's tassels and the leaf's midrib are lone
lines, yet Chrome's are whole eighths. Probed: a lone line after four
path draws (fills or strokes of any path but a lone line or a lone arc)
is D650's quad; after five it is multisampled, the quad's own samples
with no ramp. Lines drawn before the fifth path stay analytic; reading
the canvas does not reset the count; rectangles, `strokeRect`, circle
fills, text and images stay analytic however many paths came before. So
the canvas counts its path draws (`slow_paths`; the path's kind follows
Blink's line and arc builders: `moveTo` + one `lineTo` is a line, one
`arc()` of radius 1 or more on an empty path, perhaps closed, is an arc,
anything else a path) and from the fifth on strokes a lone line by its
snapped quad's samples. The mechanism is not pinned down (the count
recalls Chromium's `kMinNumberOfSlowPathsForMSAA`, but Chromium counts
only concave paths and Chrome here counts convex ones too); the rule is
what was measured.

The textures, mean absolute difference against Chrome (R/G/B/A, levels;
before → after; every change over 0.05): Valley corn 4.56/4.95/2.04/4.20
→ 0.00/0.00/0.01/0.00; Beach palm leaf 2.73/5.89/1.94/9.39 →
0.011/0.025/0.015/0.002; Seaside catch fence 11.828/11.709/9.072/5.119 →
0.148/0.117/0.088/0.029; the rock (all levels' `tRock`) 0.108/0.106/0.103
→ 0.011/0.010/0.011; the freeway and harbour signs (rounded-rect borders)
0.29-0.42 → 0.11-0.21 in R (sign-port-meridian 0.422/0.352/0.315 →
0.128/0.107/0.095); City's 2048×768 and 2048×1536 billboards
0.327/0.207/0.256 → 0.149/0.094/0.113 and 0.319/0.202/0.250 →
0.142/0.089/0.106; Mountain's canvas 0 0.11/0.11/0.10 → 0.01/0.01/0.01
and canvas 6 0.74/0.22/0.33 → 0.66/0.20/0.29; the paths probe
0.447/0.381/0.289 → 0.292/0.202/0.157. Nothing got worse; every other
texture and probe moved by less than 0.05.

## WP 6.1–6.2 decisions

## D570. The menu comes first; what the address and the store decide

2026-10-04, WP 6.2, the owner's request for the main menu. Opening
`dist/next/` with no `level=` shows the main menu over the attract camera,
as the JS game does; the level built behind it is the saved one
(`mr.level`, else Sierra), and the page downloads that level's scene
(`start_level()` tells it). `?level=…` still races at once (D432), and so
do `autostart`, `race=1` and natively `shots=` (`ui::menu_first`). The
fly camera, `freeze=1`, the material scenes, the stations and the
measurement page are untouched (none of them is a race). Changes of
default: a race started from the address now uses the saved car
(`mr.car`) unless `car=`/`autostart=` names one, and the saved Race / Hot
Pursuit choice for its level (`mr.mode.<id>`) unless `pursuit=` says;
`hq` comes from `mr.hq` (the JS default, off on touch screens) unless
`?hq=` says, on both platforms (natively it was always on). Natively the
binary with no `--level` opens the menu too, on the saved level; the
store is a file (D572), and `--level` races as before.

## D571. The screens' font is Rajdhani, the JS's `--font`

2026-10-04. The brief said "the bundled Roboto fonts"; the JS menus,
HUD and screens are set in Rajdhani (`hud.css` `--font`, Google Fonts
500/600/700), which D370 keeps for the HUD and mp_canvas already bundles,
so the screens use it: the same files as the JS page's (subset, D371),
registered with Bevy's text from mp_canvas's copies (no second copy in the
wasm). Chrome only has 500, 600 and 700, so a normal weight draws Medium
and 800/900 draw Bold, as there. Bevy draws no synthetic styles, and the
menus' headings are `font-style: italic` (Chrome's fake italic of
Rajdhani): `assets/fonts/oblique.py` bakes Skia's skew (x + y/4 about the
baseline, advances unchanged) into a copy of Rajdhani Bold (27 KB), named
"Rajdhani Oblique" so the text engine picks it by family. The M4 HUD and
the touch controls (D431) switch from Bevy's FiraMono to Rajdhani Bold by
putting that face at Bevy's default font handle; nothing else in them
changes. Characters Rajdhani lacks (the arrows, ●) are drawn from
Arimo's symbols, the fallback face of D372, as Chrome falls back for
them (`widgets::text` cuts a string into runs). If the owner wants Roboto
for the screens after all, it is `widgets::FAMILY`.

## D572. The store: `localStorage`'s keys and strings, a file natively

2026-10-04, WP 6.1 (SPEC 8.3). `ui::store::Store` is `main.js`'s
`store`: `mr.<key>`, the value as `JSON.stringify` writes it (numbers as
JS prints them: `1`, `0.7`, `1e+21`), a missing or unparsable value gives
the default. On the web it is `localStorage` (the page is served from the
same origin as the JS game, so settings, best times and controller maps
carry over); natively a JSON object of the same key → string pairs in
`$XDG_CONFIG_HOME/midnight-racer/storage.json` (`~/.config`,
`~/Library/Application Support` on macOS, `%APPDATA%` on Windows),
rewritten on each change, `$MP_STORE` to point elsewhere. `Settings` is
the JS's `settings` object with its defaults and checks; nothing is
written until a control changes, as there. The volume, track, steering,
pedal, tilt, auto gas and rumble settings are stored and shown but not yet
used: the sound (M5) and the touch modes, tilt and gamepad (WP 6.4–6.6)
read them when they come.

## D573. The widgets, and what is approximated

2026-10-04, WP 6.1 (SPEC 8.1). Sizes are the CSS's in CSS px, turned
into Bevy UI px by the page's measured scale; the media queries are
`widgets::Bp` (720 wide, 780 and 500 tall, portrait, touch), and the
two-column phone menu is a Bevy UI grid. A screen is a scrolling column
over the radial gradient, centred while it fits (`safe center`, as auto
margins). What Bevy UI has no equivalent for:
- Gradient text (the logo): MIDNIGHT takes the colour its glyphs mostly
  show (the white top of its gradient), each letter of RACER the colour
  of the accent-to-orange gradient where it stands, and the pink glow is
  left out. Drawing the logo with mp_canvas's text, as the CSS paints it,
  was tried and looked right, but it linked a second copy of the font
  stack (harfrust, read-fonts, skrifa: 1.1 MB raw, about 0.3 MB gzip), so
  the results table is laid out by Bevy's grid for the same reason (no
  text measured outside Bevy).
- The selected tab or car keeps its accent border and 1 px ring (an
  outline); its 24 px glow is left out, because Bevy draws a box shadow
  under the whole node and these nodes are translucent (it tints them).
  The primary buttons' shadows are kept (they are opaque).
- Native form controls are drawn as Chrome draws them with
  `accent-color`: the 13 px checkbox, the range slider's 4 px track and
  16 px thumb (dragged by finger or mouse), the select with a chevron,
  which opens a list under it (Chrome on a desktop) rather than Android's
  picker. No hover states; the keyboard focus ring is the gamepad's
  (`.pad-focus`), Tab and Shift+Tab move it, Enter or Space activates.
- Characters no bundled face has are drawn with mp_canvas's paths: ★
  (results), ⏭ (Next track), ♪ (the music link) and the tick; ◂ ▸ in the
  Steering choice and the touch help become ← → (Arimo has them); N₂O is
  N2O (D431).
- The music player link opens the JS page (`../../music.html`) until
  M5's player screen; natively there is no link.
Every control carries its DOM id (`btn-start`, `opt-hq`, …); level tabs,
car picks and mode buttons, which had none, are `lvl-tab-<id>`,
`pick-<car>` and `mode-<race|pursuit>`.

## D574. The session flow: level tabs, Race, the warm-up race

2026-10-04, WP 6.2 (`main.js` `loadLevel`, `startRace`, `toMenu`). A
level tab saves the level and builds it behind the loading screen, as the
JS does: the scene is torn down and the other level's export downloaded
(the page's `__mp.reload`, D394; natively a thread reads it). Race loads
the chosen level first if it is not the one built, then builds the field
(`play::Play` gains `armed`, `hold` and `stop`) and holds the countdown,
with the menu still up, until the frame's pipelines are compiled (three
quiet frames, at most three seconds: the JS's `compileAsync` race); a
second tap meanwhile does nothing. A menu-first run also builds a field
behind the loading screen and drops it when the client is ready, so the
warm-up (D390) covers the cars' pipelines and Race does not hitch. Main
menu disposes the race and its cars and puts the attract camera back at
`startS + 60`. The HUD and the touch controls hide while there is no race
or it is held.

## D575. The canvas draws the menu at up to twice the CSS resolution

2026-10-04, WP 6.2. The screens are drawn into the game's canvas, which
D437 renders at 1× on a phone without High quality: the menu's text would
be a third of the iPhone's resolution, where the DOM's was always sharp.
While there is no race (the menu, the loading screen, the controller
screen opened from the menu) the canvas is drawn at `max(JS ratio,
min(devicePixelRatio, 2))`; with a race (driving, pause, results) at the
JS's ratio as before. Pause and results stay at the race's ratio because
the race's HUD text, laid out at it, came out at the wrong size when the
ratio changed under it. The attract camera behind the menu costs up to
four times the pixels on a phone; the race is unchanged.

## D576. The loading screens

2026-10-04, WP 6.2. On the web the loading screen is the page's own (it
shows while the wasm and the scene download, which only the page sees),
now styled as the JS's `#loading` (the gradient, the logo, the bar and the
spaced-out line, in the bundled Rajdhani by `@font-face` from
`../../assets/fonts/`); the page shows it again for a level change.
Natively, and on the web under the page's, the client draws the same
screen with Bevy UI from its own state (waiting, building with the
build's progress, preparing the shaders).

## D577. The screens' part of the test bridge, and the suites

2026-10-04, WP 6.2 (SPEC 8.5; WP 6.7 owns the rest). `window.__mp` gains
`screen` and `mode` (`__game.mode`'s values), `ui(id)` (`{x, y, w, h,
visible, enabled, value, sel, z}` in CSS px, from the last frame's
layout; the touch controls' taps as `touch-reset`, `touch-camera`,
`touch-pause`), `reveal(id)` (`scrollIntoView` to the middle), `focus`,
`races` (races started, for "exactly one race"), `race.locked`, and
`stage(cmd)` with `finish` (the suites' `teleportToFinish`), `cruise` (the
score) and `padsetup`. `test/` is frozen, so the suites are adapted in
`tools/parity/e2e/` (`harness.mjs` is the JS harness's API over `__mp`:
`center` reveals a control and fails if another one is on top of it, as
`elementFromPoint` did). Request interception cannot carry Sierra's full
export (D106), so the harness answers a request for a full export over
90 MB with that level's terrain-road-sky export (Seaside's full one
passes). The page's gesture handlers now pass the
pointer's position (`gesture_at`): a tap on Race, Race again or Restart
on a touch screen goes fullscreen and asks for landscape inside the tap,
as `enterFullscreen` does. The suites' sound checks are in (D580).

## D578. Pause, results and the controller screen

2026-10-04, WP 6.2 (`#pause`, `#results`, `#padsetup`, `showResults`).
They replace the M4 card (D432): pause has Resume, End run (a cruise
only), Restart, Main menu, the volume sliders and Next track; results
have the title, the table (place, swatch, name, time, `~` for an
estimate), the stat tiles (Hot Pursuit's busts, wrecks, takedowns,
penalty and top heat; a circuit's laps with ★ on the best and the lap
record), the best line and Race again / Main menu. `showResults`' saving
is ported as it is: a winning time under `best.<id>` (`.pursuit` for Hot
Pursuit) when first and better, a cruise's score under
`bestScore.<id>`, a circuit's best lap under `bestLap.<id>`. Restart and
Race again restart the race in place (`flow::Race::restart`, a new seed
unless `seed=`), as M4 did, rather than building the field again. Esc and
P still pause and resume, Enter still races again or resumes (D432). The
M4 card's "tap anywhere" stays only on the pause screen, for a tap on no
control (the owner's phone flow from M4); on the results a tap does
nothing but on a button, as in the JS. The phone harness of M4 tapped
the middle of the screen to resume, which is now Main menu; its copy in
this package's checks taps Resume. The Controller screen is the JS's,
reached from the menu or pause when a gamepad is connected (`ui.pads`,
never until WP 6.4 reads pads; `__mp.stage({cmd: 'padsetup'})` opens it
for the tests): it lists the actions with the standard layout's labels,
the Rumble option (saved), Defaults and Done; picking a binding waits for
6.4. Esc there leaves it and keeps the race paused.

## D579. Size, and an open question on scene sizes

2026-10-04, WP 6.1–6.2. Release build, gzip: WebGPU 8.77 → 8.92 MB,
WebGL2 9.24 → 9.40 MB (+0.15 and +0.16 MB) of the 10 MB budget. The fonts add
nothing (the wasm already carried mp_canvas's bundled files; the oblique
face is 27 KB). Not decided here (D439): a level tab downloads that
level's whole export behind the loading screen, as the JS rebuilds the
world, and on a phone Coast is 210 MB; the menu could show each level's
download size on its tab or card, or a tab could only select and Race
load. Raised with the owner; until they answer, a tab loads its level, as
in the JS. Merged with main at a60cb88 (race audio, world build): main measures
WebGPU 10.00 and WebGL2 10.48 MB gzip, already over the budget, and the
screens bring them to 10.14 and 10.61 MB (+0.14, +0.13). Raised, not
decided here.

## D580. The screens with the race's sound and the client's world build

2026-10-04, merging WP 6.2 with the race audio (D513–D517) and WP 3.9's
world build (`animate`, D490–D497).
- **Gestures.** The page calls the bridge on all five of the JS's events
  (pointer-down too, D514), so the first touch on the menu wakes the
  sound as `wakeAudio` does; only the up-events (pointer-up, touch-end,
  click) carry the pointer's position for fullscreen, and the request is
  made once per tap (the three events of one tap within 0.8 s). A menu
  tap acts once: the Bevy side reads the touch, not the page's events.
  A menu-first run builds the sound's graph behind the loading screen
  with its warm-up race (D574), so the first tap starts it, and the
  sound is running by the time Race's countdown starts.
- **Settings.** The menus and `play::audio` read the same keys
  (`mr.musicVol`, `mr.sfxVol`, `mr.track`); `ui::sync_audio` keeps the
  two copies equal each frame: a slider or the track picker reaches the
  sound (`applyVolume`, and `pickMusic` for a new track), and the music
  key M (which the sound stores) reaches the sliders. A level tab plays
  that level's track when the sound is up (`if (audio.ready)
  pickMusic()`), Next track on the pause screen calls it, the menus'
  buttons make the JS's click (`uiClick('click')`; Race, Race again and
  Restart make the sound's own "start"), and Main menu makes `toMenu`'s
  calls (unpaused, the engine idle, no rivals, open acoustics). The sound
  polls and publishes `__mp.audio` on the menus too, so the music plays
  on there. End run (a cruise from the pause screen) unpauses the sound,
  as `btn-end` does. Every race start has its own number
  (`flow::next_start`), so a race built afresh from the menu makes
  `startRace`'s sound calls as a restart does.
- **World build.** A level tab tears the scene down through
  `unload_scene`, which enters `AppState::Waiting`; `animate::reset_world`
  runs there and `drive_build` builds the new level's world when it is
  one world generation builds (Sierra) and stands down otherwise. With
  `?world=gen` the native loader reads no file for such a level (the
  build is the scene) and the page downloads none (`generates_scene`).
  Checked both ways, natively (`--query uiscript=…`, a smoke hook that
  activates menu controls in turn and exits once racing) and on the web
  (`tools/parity/e2e/level-switch.test.mjs`): Sierra → Seaside → Sierra,
  then a race, with and without `world=gen`.
- **Phones.** The M4 phone check is `tools/parity/e2e/phone.cjs` (resume
  by the Resume button, D578), the menu-to-race one
  `tools/parity/e2e/phone-menu.mjs`.

## Wasm size decisions

Part of roadmap WP 9.1's wasm size work, brought forward because main went
over SPEC 6.6's 10 MB budget (after gzip) once the client linked Level 1's
world generation (D491), race audio and the menus. Measurements and the
attribution are in `BASELINE.md`, "Wasm size: where the bytes go".

## D670. No `bevy_post_process`

2026-10-04. `mp_game` asked Bevy for `bevy_post_process` (D100's list),
but the client's post chain is three's, ported in `render::post`, and no
camera ever carries Bevy's `Bloom`, `DepthOfField`, `MotionBlur`,
`AutoExposure` or effect-stack components. The feature only made
`DefaultPlugins` add `PostProcessPlugin`: render-graph systems, pipelines
and WGSL that did nothing (and `MsaaWritebackPlugin`, which only acts when
a second camera draws into the same target; the client has one camera).
Without it the gzipped wasm is 0.15 MB smaller (WebGPU 10.00 to 9.85 MB,
WebGL2 10.48 to 10.33 MB). Seaside's 29 stations on both backends and
Sierra's 06250 to 07500 stations through the web build are the same
pixels as before (the one difference, 06250-high, is there between two
runs of the same build too).

## D671. Debug and trace logging compiled out of release wasm

2026-10-04. On the web, Bevy's `LogPlugin` filters at `info` (plus its
default `wgpu=error,naga=warn`), and the `RUST_LOG` override it reads is
an environment variable the browser does not have, so no `debug!` or
`trace!` from Bevy, wgpu, naga or the client can print there. The wasm
target of `mp_game` now depends on `tracing` and `log` only to turn on
their `release_max_level_info` features, which compile those calls and
their strings out of builds without debug assertions. 0.04 to 0.05 MB
after gzip (WebGPU 9.85 to 9.81 MB, WebGL2 10.33 to 10.28 MB). Native
builds keep every level and `RUST_LOG`. Turning `info` off as well
(`release_max_level_warn`) would remove messages the console shows today
(adapter, window, warm-up), so it is not done.

## D672. The client's plugins: `DefaultPlugins` less Bevy's 2D sprites

2026-10-04. `bevy_ui_render` forces `bevy_sprite` and `bevy_sprite_render`
on, and `DefaultPlugins` then adds `SpritePlugin` and `SpriteRenderPlugin`
(sprites, 2D meshes, colour materials, tilemaps, 2D text and their
pipelines and shaders), none of which the client draws. Disabling them in
the group does not take their code out: a plugin group holds every plugin
as a `Box<dyn Plugin>`, so the plugin's `build` and all it registers stay
linked. `mp_game::plugins::ClientPlugins` is therefore `DefaultPlugins`
for exactly the features `mp_game` turns on, in its order, natively and
on the web (the native build keeps `TerminalCtrlCHandlerPlugin` and
`PipelinedRenderingPlugin`), with the two sprite plugins replaced by
`UiSpriteSupport`, the two things `UiRenderPlugin` reads from them:
`TextureAtlasPlugin` (the `Assets<TextureAtlasLayout>` resource) and, in
the render world, `SpriteAssetEvents` filled by `extract_sprite_events`
(with which the UI drops the bind groups of images that changed). The
gzipped wasm is 0.25 MB smaller (WebGPU 9.81 to 9.56 MB, WebGL2 10.28 to
10.01 MB). Checked: Seaside's stations on both backends and Sierra's
stretch the same as before (one WebGL2 pixel one level apart, inside that
backend's run-to-run noise); the phone harness (touch driving, drift,
brake, pause and resume, and the autopilot race to the results) passes,
its HUD, touch controls and results screen as before; `cargo xtask parity
materials` (native) 23 stations, 0 over the limits, worst mean 0.150 as
before; the workspace tests pass. Frame time (`rust-perf.mjs`): the same
on Sierra and on Seaside's WebGPU; on Seaside's WebGL2, uncapped, the mean
frame is 1.7 ms against 1.5 and the 95th percentile 10.4 against 6.6 (the
median 0.9 against 1.0), but with vsync on both hold 16.7 ms at every
percentile, so it is WebGL2's uncapped pacing, not added work
(`BASELINE.md`).

The cost is a rule: **a Bevy feature that brings a plugin now needs that
plugin added to `ClientPlugins`** at its place in `bevy_internal`'s
`default_plugins.rs`, since `DefaultPlugins` no longer adds it; the
module's header says so. The menus (WP 6.1 and 6.2) and WP 3.9's client
work turn on no Bevy feature and add no Bevy plugin (both agents
confirmed). At the move to Bevy 0.20 (D100) the list is redone from that
version's `default_plugins.rs`.

## D673. The web-release profile and wasm-opt stay as they are

2026-10-04. D4 left `wasm-opt -Oz` against `-O3` to be measured once
there were frames. Measured on a60cb88 with D670 to D672 (`BASELINE.md`,
"Wasm size: where the bytes go"): `-O3` and `-O4` are 0.14 to 0.18 MB
larger after gzip and no faster on any of the four flights (Sierra and
Seaside, WebGPU and WebGL2); `-Oz --converge` saves 4 to 5 KB for six
times the wasm-opt time. `strip = true` changes nothing in the shipped
file (wasm-opt already drops the name section). So the profile keeps
`opt-level = 3`, fat LTO, one codegen unit, `panic = "abort"`, and
wasm-opt keeps `-Oz`. The settings that would change size a lot (`opt-
level` "s" or "z", `simd128`) are the owner's to choose, D674.

## D674. Open: further size options, for the owner

(Item 1 decided in D675: `"s"`, and the budget raised to 16 MB.)

2026-10-04, not decided: each is a choice the owner asked to make (D439)
or costs something. Sizes are after gzip on the merged build (WebGPU 9.29
MB, WebGL2 9.76 MB, under SPEC 6.6's 10 MB) unless said otherwise;
details and frame times in `BASELINE.md`.

1. **`opt-level = "s"` for web-release**: -1.33 MB WebGPU, -1.41 MB
   WebGL2; pictures identical, the simulation's wasm tests pass; costs
   CPU: Sierra ready 0.5 to 0.6 s later (the world build in the page),
   median frames +0.1 to 0.2 ms where they are CPU-bound.
   **`opt-level = "z"`**: -2.36 and -2.50 MB; ready 1.5 s later, median
   frames +0.3 to 0.6 ms, and five single pixels of Seaside's WebGPU
   stations differ (reproducibly). Either could also be applied to some
   crates only (`[profile.web-release.package.<crate>]`, for example
   Bevy's UI and text, naga, or `mp_worldgen` without the simulation),
   not measured.
2. **`-C target-feature=+simd128`**: -0.16 MB on each build, no frame-time
   change, pictures identical, simulation tests pass in wasm. Safe for the
   WebGPU build (every WebGPU browser has wasm SIMD); the WebGL2 fallback
   would stop loading on Safari before 16.4. Needs its own target
   directory or flags plumbing like D391's, since RUSTFLAGS change the
   build cache.
3. **Brotli** instead of gzip for `dist/`: WebGPU 5.82 MB, WebGL2 6.12 MB
   (from 9.27 and 9.73 by gzip -9); the dev server would send `.br`, and
   the budget would be measured in brotli. **Zopfli-made gzip**: 8.86 and
   9.31 MB, decoded by every browser, a change to `precompress` only.
4. **One font stack.** `mp_canvas` shapes and hints with harfrust 0.13 and
   skrifa 0.46 (read-fonts 0.43, D152); Bevy's text uses harfrust 0.6 and
   skrifa 0.42 (parley) and skrifa 0.44 (swash). Moving `mp_canvas` to
   parley's versions would drop one copy of each, about 0.30 MB, but is a
   port of its text code to older APIs with the texture gates rerun
   (autohinting changed between those versions); or wait for Bevy 0.20's
   parley.
5. **A patched Bevy** (`[patch.crates-io]` for two crates) could drop what
   no feature turns off: the 2D core pipeline that `CorePipelinePlugin`
   always adds (about 0.08 MB), naga's GLSL front end and its
   preprocessor, which `bevy_shader` turns on for wasm through naga_oil's
   default features (about 0.09 MB), the UI debug overlay and the text
   editing and clipboard systems (about 0.07 MB): about 0.25 MB, against
   carrying a patched Bevy until 0.20.
6. **Fonts out of the wasm**: the 17 bundled files are 0.65 MB of it after
   gzip. As files fetched beside it they download the same bytes, unless
   only the fonts a level's signs use are fetched.
7. **World generation as a second wasm**, loaded when a level is built:
   `mp_worldgen`, `mp_canvas`, its text stack and the fonts are about 1.5
   MB of the first download today (Sierra's scenery only); more as levels
   are built in the client: Seaside +0.03 MB, Coast +0.20, Desert +0.14,
   Streets +0.10, the Cruise nothing (City only), +0.44 MB for all.
   Linking all of them into the one wasm instead, with options 1 to 4
   untaken, would bring WebGPU to about 9.7 MB and WebGL2 to about 10.2.
8. **The material test scenes on the web** (`?mat=`, `matscene`, used
   natively only by `cargo xtask parity materials`): about 0.02 MB.

## D675. `opt-level = "s"` for the web, and a 16 MB budget

2026-10-04, the owner's answer to D674's first option ("opt s sounds
fine but i also dont care if we raise it. seems tight if we do bigger
levels"). The web-release profile takes `opt-level = "s"`; native release
builds keep `opt-level = 3`. Measured on b68d5ad with the change, gzip -9:
WebGPU 9.29 → 8.09 MB, WebGL2 9.76 → 8.47 MB (raw 24.36 and 25.30 MB). The
costs are D674's: Sierra ready 0.5 to 0.6 s later, median frames 0.1 to
0.2 ms longer where they are CPU-bound, pictures identical.

SPEC 6.6's download budget goes from 10 to 16 MB after gzip
(`xtask/src/size.rs`, which CI enforces). 10 MB was a figure set when the
spec was written, not a measured limit, and every level's client world
generation (+0.44 MB, D674 item 7) plus the screens still to come would
have kept the build at its edge. Level content is not in the wasm (it
is scene data or generated in the client), so bigger levels grow it only
by their generation code. D674's other options (`"z"`, `simd128`,
brotli, one font stack, a patched Bevy, fonts or world generation outside
the wasm) stay open and are not needed for the budget.

## D676. The menu switches levels instantly

2026-10-04, the owner on D579 ("i dont like loading a full level on menu
select. menu should allow instant switching, maybe we load small flyover
sections for all levels at once"). A deviation from the JS, whose level
tab rebuilds the whole world: in the Rust client a level tab or card only
selects, and the menu's attract camera shows a short section of that
level's route, prepared for every level when the menu opens, so switching
is immediate. The whole level is built or downloaded when Race is
pressed, behind the loading screen. How the sections are made (generated
in the client from each level's world generation, which now fits the
budget, D675, or cut from the exports), their length, and the memory they
may hold on a phone are for the package that does it; recorded in
DEVIATIONS.md when it lands.

## D677. Brotli for the web build

2026-10-04, the owner ("turn on brotli"; D674 item 3). `cargo xtask web
--release` writes a `.br` (quality 11, 16 MB window) beside the `.gz` for
the wasm and its JS, and `tools/serve.py` sends the `.br` with
`Content-Encoding: br` when the request offers it, the `.gz` otherwise.
Browsers offer brotli only on https, so phones on the tailnet front get
it and plain-http loads keep gzip. `cargo xtask size` reports both; the
budget (16 MB, D675) stays on gzip, the encoding every load can fall
back to. Scene files are not precompressed: they are the parity cache's
exports, read only when a level is not built in the client, and their
delivery is still open (D439).

## Seaside Raceway and the Night City Cruise in the client

The client halves of WP 7.4 and 7.5: D595's and D632's lists.

## D680. Seaside Raceway is built in the client: `crate::levels`

2026-10-04, WP 7.4 (D595's list). `animate::generated` is true for
`seaside` (and `cruise`, D683), so the client builds Seaside Raceway for
its animators, and with `?world=gen` draws that build without the
download. What Seaside needs beyond Level 1's setup is in a module of its
own, `crates/mp_game/src/levels/` (`animate` gains one line in
`generated`, a call to `levels::new_build` at the top of `new_build`, a
wait on `levels::inputs_ready` before a build starts, and the countdown of
D682): the build is `tests/seaside_animators.rs`'s, `seaside::prepare`
with the survey, `World::with_level_data` with the same
`Arc<SeasideData>`, `TerrainSetup { ground_color:
seaside_ground_color(survey), photo: GroundPhoto::seaside(..) }`, and a
scenery factory naming Raceway alone (D498's rule: `scenery::PORTED`
would link every level's modules).

- **The survey** is the one `make_track` parses for the Track (the page's
  download of `assets/seaside/survey.bin`, the file natively): it hands a
  copy to `levels::seaside::survey_parsed`, which keeps it across reloads.
  Natively `native::load` read the survey only when it also read the
  export; it now reads it first, so `?world=gen` gets it too. A failed
  survey still starts the build, whose first job (the Track) fails and
  says so (D590).
- **The photo** (`src/levels/seaside/photo.jpg`, 1843 × 2160) is the
  caller's to decode (D234). On the web the page decodes it as the scene
  exporter read it (`tools/parity/lib/scene-page.js`: an `Image` drawn on
  a canvas, `getImageData`) and hands the bytes in (`load_photo`;
  `photo_failed`; `has_photo`, so it is fetched once). Natively, D681. It
  is kept once decoded (an `Arc<Texture>` every drawn build shares,
  16 MB), so a reload, or the menu's sections (D740+), do not decode it
  again. It is fetched only with `?world=gen`: with the export drawn, the
  build is for the animators, which never read the photo, and a 1 × 1
  blank stands in (the texture keeps its place in the numbering, so the
  build's shape is the export's and the edits address it).
- The wasm grows by Raceway's world generation: 8.12 MB (WebGPU) and
  8.50 MB (WebGL2) after gzip, against 8.09 and 8.47 (D675).

## D681. The photo natively: the `image` crate's JPEG decoder

2026-10-04, WP 7.4. The native client decodes `photo.jpg` with `image`
0.25 (JPEG only, zune-jpeg), a dependency of the native target alone, so
the wasm does not link it (the browser decodes on the web, D680). It is
not Chrome's decoder: against Chrome's decode in the export it differs by
0.278 levels mean absolute, at most 4 levels, and 0.012 % of the bytes by
more than 2 (IDCT and chroma upsampling). `levels::seaside::tests`
holds it: byte for byte against the golden's SHA-256 if it ever matches,
else under 1 level mean against the cached export. The pictures that gate
the level are the web build's, whose decode is Chrome's (D684).

## D682. The start lights follow the race's countdown

2026-10-04, WP 7.4. `Race.update` calls `world.onCountdown(started ? -1 :
this.countdown)` every frame of a race. The client's equivalent:
`levels::RaceCountdown` (PostUpdate, before `run_animators`) holds the
race's `countdown` while its state is `Countdown`, -1 after, and `None`
with no race (the menu, the fly camera), when the JS does not call it;
`run_animators` appends `WorldBuild::countdown(cd)`'s edits after
`WorldBuild::update`'s, as D595 asked, and applies them as D493 applies
any: the five lamp materials' `emissiveIntensity` goes to their animation
blocks (D490), so in a Seaside race 6 materials have blocks (the road's
`uWet` and the five lamps). The countdown is the simulation's at the last
tick, as the HUD's numbers are. Checked in the web build, downloaded
export and `?world=gen` alike: the lamps dark at the start, four columns
lit at 0.89 s to go (`ceil((4 - 0.89) × 5 / 4) = 4`), all dark at GO.
The JS e2e `circuit` test's count (`lampMats`) has no Rust bridge yet;
the logic itself is held step by step by `tests/seaside_animators.rs`
(D594).

## D683. The Night City Cruise in the client

2026-10-04, WP 7.5 (D632's list). The loop needs nothing beyond Level 1's
setup: City is its only scenery, and `animate::level1_scenery` names it,
so `generated("cruise")` is the whole change. The build has 12 animators;
10 materials get animation blocks, as D632 counted. With the export drawn,
its 52 nodes exported hidden are spawned hidden (D498) and the cut-off's
visibility edits now show and hide them; with `?world=gen` the build's
scene starts all visible and the first frame's cut-off hides what is far.

## D684. The L4 gate on Seaside's and the loop's stations, from the web build

2026-10-04, WP 7.4 and 7.5, D496's method: `rust-web-stations.mjs
--level <id> --query world=gen` (no download; the photo through the
request interception), then `cargo xtask parity shots` against the JS run
`a`. **Seaside Raceway: all 29 stations within SPEC 12's limits** (14
places × chase and high, and the attract view), median 0.21 mean ΔE00 and
0.49 block 95 %, worst 0.925 / 5.01 (the attract view under the
start-finish bridge, where the banner's "MIDNIGHT RACER" is set in the
bundled Roboto and the JS shot in the machine's Arial, so one letter falls
behind a post: D370). **Night City Cruise: all 115 stations within the
limits** (57 places × 2 and the attract view), median 0.34 / 0.98, worst
0.916 / 4.14 (03000-high).

## D685. The races, keyboard and autopilot

2026-10-04. In the web build (WebGPU, headless Chrome, `?world=gen` and
the downloaded export): Seaside Raceway, W held from GO gives 13 m/s and
200 m in 8 s; with `autodrive=1&timescale=4` the three laps run to the
results (4:27.47, laps 1:31.21, 1:28.27, 1:27.99, results with the lap
row). Natively the same with `shots=` (countdown, race, results). The
Night City Cruise: W and autodrive both drive the loop, the score
counting. The e2e suites that touch these levels pass against the build:
`level-switch` (Sierra → Seaside → Sierra and a race, downloaded and
`?world=gen`, where Seaside is now built rather than downloaded) and
`race-flow` (11 tests, the cruise's End run among them).

## D686. Budgets: memory and frame time on Seaside and the loop

2026-10-04. Wasm size: D680. Wasm memory and frames from
`tools/parity/rust-perf.mjs` (WebGPU, the dev machine's RTX 3060, uncapped
frame rate, 30 s flight, the machine shared: load averages given):

| Run | ready | p50 / p95 / max ms | wasm MB, load → reloads | load |
|---|---|---|---|---|
| Seaside `?world=gen` | 2.7 s | 3.0 / 7.2 / 41 | 205 → 236 (10 reloads, +0) | 10.0 → 10.5 |
| JS Seaside | 1.3 s | 0.4 / 1.5 / 737 (242 over 50) | JS heap | 10 |
| Cruise `?world=gen` | 5.4–6.7 s | 2.9 / 7.4 / 38 | 438 → 479–490 (10 reloads, +0) | 4.3–9.1 |
| Cruise export, no build (`world=off`) | 4.2 s | 2.6 / 7.9 / 37 | 390 → **562** | 4.3 |
| Cruise export with the build (default) | 5.2 s | 3.1 / 8.4 / 34 | 443 → **615** | 6.4 → 11.6 |
| JS Cruise | 2.3 s | 0.5 / 1.7 / 105 (150 over 50) | JS heap | ~6 |

Seaside's downloaded path (the 38 MB export and the build for the
animators) stays at 192 MB. The race pages' figures agree: Seaside 219
(`?world=gen`), Cruise 448 at load and 512 after two reloads with the
race's cars and sound.

So the loop's default path is over SPEC 6.6's 512 MB on a second load,
and was before this package (562 MB with no world build; the build adds
the animators' 50 MB). Its `?world=gen` path, with no 181 MB export to
download and parse, stays under the budget and does not grow over ten
reloads, at the same frame times. The animators cost nothing measurable
(the loop's per-frame edits, D632, are mostly unchanged values). Not
decided here, the owner's (D439, D492): whether the client should build
its levels instead of downloading them. For the loop the memory now says
it must, or the export path must lose its CPU copies; for Seaside either
path fits.

## D687. The lap and cruise lines of the plain HUD

2026-10-04, WP 7.4's lap HUD and WP 7.5's cruise scoring HUD. The plain
HUD (`play::hud`, D431, a stand-in until M6's styled HUD) carried `LAP
n/of` and the cruise's score. It now carries what `HUD.js` `update` shows
in `#hud-lap` and `#hud-cruise`: on a circuit the lap's time (`time -
lapStart`, none once finished) and `BEST LAP` (the fastest lap so far);
on the cruise the score (`Math.floor(…).toLocaleString()`), `×mult` with
its timer as a ten-segment bar (`clamp(multTimer / 6, 0, 1)` when the
multiplier is over 1, as `#hud-mult-fill`'s width), the distance in miles
(`this.mph` is true) and the best (`max(hud.bestScore, score)`, the
stored `bestScore.<level>` read at each start as `startRace` sets it). The
layout, fonts and styling are M6's (WP 6.3).

## M7 client: Desert Run and Downtown Streets decisions

## D720. Desert Run and Downtown Streets are built in the client

2026-10-04, roadmap M7 (WP 7.3's and 7.2's client halves). With the
budget at 16 MB (D675), the client builds Desert Run and Downtown Streets
as it builds Sierra (D491, D492): `animate::generated` names the three, and
each level's scenery is named in a file of its own
(`crates/mp_game/src/levels/desert.rs`, `streets.rs`: `Desert` and
`Streets`, each the level's only module, D550, D610), not through
`scenery::PORTED` (D498). Both are numbered as their exports (D551, D613),
so as for Sierra: with `?world=gen` the page downloads nothing and the
loader draws the build, and by default the export is drawn and the
build's animators run over it (`?world=off` turns them off). The menu's
level tabs build them too (D580). The wasm grew 0.15 MB after gzip
(WebGPU 8.09 to 8.24 MB, WebGL2 8.47 to 8.62 MB; D674 item 7 estimated
0.24), under SPEC 6.6's 16 MB.

## D721. What the two levels' animators needed of the client

2026-10-04. Every edit the two levels' animators make is now applied (no
"not applied" line natively at `mp_game::animate=debug`), which closes
D556's and D614's lists, whose kinds D500 and D501 drew:

- **The train's spot light** (Desert; D553, D556). `SceneIndex` keeps the
  scene's first spot light (the one the loader puts in `Lighting`, D456's
  scene spot) and its target, the next sibling; its `Light` edit sets the
  colour and intensity (0 by day, 90 at night) and the moved nodes' world
  positions set its position and direction, as three reads `matrixWorld`
  and `target.matrixWorld` (`levels::desert::follow_spot`). `Lighting` is
  written only when a value changed. Other levels' animators make no
  `Light` edit, and the sun and hemisphere lights stay the sky's.
- **The flicker points' clock** (Desert's nine FlickerPoints, D553): the
  animator's `uTime` goes to the block's texel 4.x like the pools' (the
  `FlickerPoints` slot), and the vertex stage reads it through
  `kind_uniforms()`, which without a block still gives the scene-wide clock
  as before (D293), so the material test scene is unchanged. Their colour
  edits (`colour × k`) were already block colours.
- **A plain material's opacity** (Desert's headlight sprites and flame,
  Coast's pools and sprites): the client-coast agent's commit (D701),
  cherry-picked here so both branches carry the same change.
- **A plain Points' size** (Streets' phone flashes, D612): the block's
  texel 4.y (`(Points, "size")`, starting from the exported size, 1 if
  none), read in the points' vertex stage under D701's `PLAIN_ANIM`.

The train, its sprites and the tumbleweeds (Desert), the elevated train,
the neon clock, the steam's clock and scale, the barrier flashers and the
aircraft lights (Streets) are transforms, instance matrices, colours and
kind uniforms the shared path already applied (D493, D490).

## D722. Streets' build frees each chunk builder as it emits it

2026-10-04, after measuring the client's build. Wasm memory never shrinks
(SPEC 5.5), so a build's transient peak is the level's high-water mark.
Measured natively with a counting allocator, Streets' scenery job (D610)
peaked at 452 MB of heap with 28 MB before it and 169 MB after it: its
ten chunked `GeoBuilder` sets hold every vertex as JS doubles, and
`emitAll` built each one's `Float32Array` geometry while all of them were
still alive. `Chunks::take` now hands over a builder and leaves an empty
one with the same attributes in its place, and `emit` and `emitAll`'s own
loops take each builder as they build it, so its doubles are freed once
its geometry exists. The JS never reads an emitted builder again (the
billboards' posts written after `emitAll`, D610, still go into a builder
nothing emits), so the scene is unchanged: `tests/streets.rs` with the
cache (the whole digest) and `streets_animators` pass. The job's peak is
354 MB; in the web build Streets' wasm high-water mark went from 492 to
394 MB after load (with `?world=gen`). Not done: keeping `GeoBuilder`'s
arrays as `f32` from the start would halve the rest (the JS rounds them to
f32 only in `build`, and nothing reads them back, so it would be exact),
but `GeoBuilder` is shared with City and Harbor (WP 3.3), so it is left
for a package that owns it.

## D723. The gates for Desert Run and Downtown Streets in the client

2026-10-04, the web build (release, WebGPU, 1280 × 800) through the
registered server, `?world=gen`, the machine shared with other agents
(load averages given per run; the GPU's 12 GB was at times nearly full
with other agents' Chrome runs, which made a station flight fail with
`VK_ERROR_OUT_OF_DEVICE_MEMORY` until they finished).

- **L4 stations** (`rust-web-stations.mjs --server --query world=gen`,
  `cargo xtask parity shots`): **Desert 63 stations, 0 over the limits**,
  median 0.155 mean ΔE00 and 0.370 block 95 %, worst 0.268 / 0.778
  (03500-high; D500 from the export: 0.681 / 1.564). **Streets 43, 0
  over**, median 0.336 / 1.001, worst 0.755 / 5.171 (02500-chase). Against
  the same flight of the export without the animators (`world=off`, D501's
  numbers again: 0.489 / 1.812) the difference is the lettering: the
  build's signs are drawn by `mp_canvas` with the bundled fonts (D370),
  the JS shots and the export with the machine's fonts (D613: up to 36
  levels in the glyphs); the neon and the banners light up the difference
  maps and nothing else does. A spot check of the WebGL2 build (three
  stations each): 0 over, worst 0.407 / 1.282. The material scenes
  (`cargo xtask parity materials`): 33, 0 over, worst 0.150 as before.
- **The race** (`tools/parity/e2e/built-levels.test.mjs`): from the menu
  with the level saved, `world=gen`, the autopilot racing 8 s and the
  finish staged, the results screen, no page error, for both levels; and
  the level tabs Desert → Streets → Desert, then a race. Natively the
  race runs on both with the animators (the train rolling beside Route 66
  at night, its spot light following it).
- **Size**: 8.24 MB (WebGPU) and 8.62 MB (WebGL2) after gzip, of 16.
- **Memory** (wasm high-water mark, budget 512 MB on phones): Desert 309
  MB after load (`world=gen`; the export path: 243 after load, 351 on
  reloads), 320 MB after a race; Streets 394 MB after load (492 before
  D722), 404 MB after a race; ten level switches between them 426 MB at
  every one (no growth). The JS heap peaks were 127 MB (Desert) and 461
  MB (Streets).
- **Frame time** (`rust-perf.mjs`, fly at 60 m/s, 60 s, uncapped):
  Desert `world=gen` (load 10.3 → 12.9) p50 / p95 / p99 3.3 / 9.1 / 15.2
  ms, 4 frames over 50 ms (all in the first 30 s, at s 1,089 to 1,724,
  load-dependent: none in the same flight of the export at load 38 → 17,
  3.5 / 6.9 / 9.6); Streets `world=gen` (load 11.5 → 22.1) 3.9 / 9.2 / 15.0,
  none over 50 ms in the first 30 s, 6 later. The JS game on this machine
  (BASELINE.md, WP 0.8) flies them at 1.5 to 2.3 ms of CPU a frame, so the
  Rust client is not yet "no worse" here, as on every level (BASELINE.md,
  WP 2.6 on); the animators cost under 0.2 ms a frame (D497).
- **Load**: ready 5.0 s (Desert) and 4.5 to 6.7 s (Streets) from
  navigation including the wasm, 2.2 to 3.4 s on reloads; the JS loads to
  the menu in 3.9 and 3.2 s. Streets' first load is the slower.

Left: L5 (the owner's review and drive on the phone); the font
difference in the lettering is D370's choice, not this package's.

## Coast Highway client decisions

## D700. The Coast Highway is built in the client

2026-10-04, M7 (WP 7.1's client half). With the wasm budget at 16 MB
(D675), Level 2's world generation is linked into the client as Level 1's
is (D491, D492): `animate::generated` names "coast", and `animate`'s build
takes the level's scenery from `levels::coast::scenery` (Coast, Beach and
Harbor by name, D498's rule; no other module). Nothing else is
level-specific: the stages are Level 1's (`LevelSetup` with no plan, as
`tests/level2.rs` builds it). So, as on Sierra, the client builds Coast
behind the loading screen on every load, runs its 14 animators (D537,
D543) on whichever scene it draws, and with `?world=gen` draws its own
build without the 210 MB download. The two scenes number alike (D536), so
the animators run over the export too (the drawn scene's shape is checked
against the build's, D491). The build is 1.2 s natively (release) and 3.1
to 3.3 s in the web build on the dev machine. Wasm after gzip: WebGPU 8.09
→ 8.22 MB, WebGL2 8.47 → 8.60 MB (D674's estimate was +0.20).

What WP 7.1's client to-do still listed is done: the kinds were drawn
from the export (D499); the wheel's multi-material mesh without groups
draws nothing (the loader makes one draw per group, so none, as three);
every edit Coast's animators make is applied: transforms of the beam's
pivot, the Ferris wheel and the harbour boats, the glow sprite's scale,
instance matrices and colours, material colours, `emissiveIntensity`,
Surf's and the beam's uniforms, the beach surf's map offsets, the plain
materials' opacity and the `Points` attributes (D701). The Sea's `uTime`
and `uOff2` edits are left to the client's `lighting::Anim`, which
computes the same values from the same dt (D495).

## D701. The animator path: a plain material's opacity, `Points` attributes, and writes into the mesh slab

2026-10-04. Three additions to the shared path (`animate`, `convert`,
`render::material`, `three_material.wgsl`), which no other level's
animators used before (Desert's opacity edits use the first; its agent
took the commit):

- **Opacity of three's own materials** (Coast's lamp pools, the
  lighthouse glow, the beach surf, the boats' lights). The kinds Standard,
  Physical, Lambert, Basic, Line, Sprite and Points take `opacity` in the
  animation block's texel 4.x (D490), which starts at the exported opacity
  (1 if absent); the shader reads it under a new `PLAIN_ANIM` def (no
  patch, the sprite, plain points) when the material has a block, so a
  material without one is unchanged. The lamp pools' opacity, `0.42 ×
  smoothstep(0.08, 0.5, night)`, is what the three sunrise stations needed
  (D499).
- **`Points` attribute edits** (the boats' running-light positions, 21
  points; the port's glow colours, 80). Unindexed `Points` geometries of
  at most 4,096 points keep their CPU copy (`convert::KEEP_POINTS`), and a
  geometry value goes to its point's four quad vertices; a colour edit of
  three components goes to Bevy's four.
- **Writes into the slab, not the asset.** Editing the Bevy mesh asset
  (how the flag's cloth was written, D497) makes Bevy re-extract and
  re-upload the mesh. On the web (Chrome, WebGPU) that left GPU memory
  behind every frame: a Coast flight with `?world=gen` grew from 1.8 to
  6.1 GB of VRAM over its two minutes (one run hung another agent's
  WebGPU work out of memory), against 1.8 GB flat with the attribute edits
  skipped, 1.6 GB with the animators off, 1.6 GB on Sierra and 830 MB flat
  natively. Now the animator path keeps its own copy of each edited mesh
  (`KeptMesh`), and its packed vertices go into the mesh's slice of Bevy's
  vertex slab at extraction (`MeshWrites`, `write_mesh_updates`), as the
  instance streams are written (D497); the asset is never modified. The
  same flight holds 1.8 GB. The 67 Coast stations give the same pictures
  either way (and differ from a run with the attribute edits skipped at
  38 stations, the glows), and Sierra's stations 00250 to 02500 are within
  the limits as before.

Other per-frame asset edits left in the client, for whoever meets the
same growth: `play::models` edits a car material when the race's light
slots are all taken (D456's fallback), and a new instance stream (a new
buffer) is made when an InstancedMesh's drawn count changes (D497). Neither
happens on Coast's flights or race.

## D702. The L4 gate on Coast's stations, with its animators

2026-10-04. `rust-web-stations.mjs --server --level coast`, WebGPU, 1280 ×
800, frozen, against the JS shots of the cache:

- `?world=gen`: **67 stations, all within SPEC 12's limits**, median 0.258
  mean ΔE00 and 0.575 block 95 %, worst 0.865 mean and 3.073 block 95 %
  (07000-high, the harbour from above).
- The export with the animators (the default): 67 within, median 0.229 /
  0.504, worst 0.748 / 2.747 (07000-high). The build's canvas textures are
  `mp_canvas`'s, hence the small difference.
- The three sunrise stations that waited for the lamp-pool updater (D499,
  commit 46b2e64): 04000-chase 5.16 / 30.22 → 0.554 / 0.723, 04250-chase
  3.98 / 23.59 → 0.303 / 0.597, 04500-chase 2.72 / 22.24 → 0.247 / 0.470
  (`?world=gen`; the export path 0.538 / 0.726, 0.287 / 0.595, 0.229 /
  0.445).
- WebGL2, seven stations (night, sunrise, the harbour): all within, the
  same numbers as WebGPU to 0.003, but for the first station flown,
  00250-chase (0.208 / 0.464 against 0.120 / 0.287).

A race works: `?level=coast&world=gen` with the autopilot (`phone.cjs
desktop`) runs from the countdown to the results.

## D703. Coast's budgets: memory, GPU memory, frame time; the open question

2026-10-04, the web build, headless Chrome on the dev machine (RTX 3060),
1280 × 800, high quality. Details in BASELINE.md, "Coast Highway built in
the client".

- **Wasm memory** (SPEC 6.6: under 512 MB on phones, no growth over ten
  level switches). `?world=gen`: 456 MB after load, 512 MB after each of
  ten reloads (no growth after the first). The export path, with the
  build: 456 MB after load, 663 MB after each reload (the export's copy and
  the build's high-water marks no longer coincide; before Coast's build,
  D394 measured 436 then 528 to 636). A race: 466 MB either way. So the
  phone budget is met only by building the level in the client, and then
  at its edge; WP 2.6 measured 929 MB as the native process's RSS, not
  the wasm memory.
- **GPU memory** (not budgeted in SPEC 6.6): a Coast race 1.36 GB of VRAM
  as Chrome reports it for the tab, a flight 1.8 GB, 2.3 GB over ten
  reloads, before D701's fix 6.1 GB and growing. Phones share far less
  memory; worth watching when the owner runs the phone pages.
- **Frame time.** The machine was overloaded (load average 21 to 190 on 24
  cores, swap full, other agents' browsers on the GPU), so the numbers are
  indications. Full route, uncapped: `?world=gen` p50 5.9 ms, p95 28.6, p99
  46.7 (load 21 → 46); the export without animators 4.2 / 8.5 / 15.7 (36 →
  58); the JS game 1.2 / 6.0 / 15.7 (47 → 36). Alternating 60 s flights
  with the animators on and off at load 100 to 190 do not separate them
  (medians within 0 to 1.4 ms either way, p95 from 8 to 49 ms in both).
  Natively the whole animator path is 0.10 ms a frame on Coast (the
  updaters 0.024 ms, 124 edits a frame). The gap to the JS game is the
  renderer's, as BASELINE.md's earlier sections record; SPEC 6.6's "no
  worse than JS" is not met on this desktop for Coast, as for Sierra.

**For the owner (D439, D492):** whether the game should build its levels
in the client instead of downloading the exports. On Coast building saves
the 210 MB download and keeps the wasm memory at 512 MB on reloads rather
than 663; the pictures are within the limits both ways. Until decided,
the default still downloads the export, and now also builds the level for
its animators.

## D678. The client builds every level by default

2026-10-04, the owner ("yes make building in the browser the default"),
closing D439's delivery question and D686, D703 and D723's. Every level
is `animate::generated` (D680, D700, D720), so with no `world` parameter
the client builds the level it shows, natively and on the web, and
downloads no scene: what the JS game does, which builds its world in the
page. `?world=export` draws the exported `.mrscene` from the parity
cache instead, a parity tool now (as do `?world=off`, an explicit
`?scene=` and `?mat=`); `?world=gen` is the default and still accepted.
The measurements behind it: wasm memory after reloads, built against
downloaded, Coast 512 against 663 MB, the Cruise 479–490 against 615 MB
(phone budget 512 MB, SPEC 6.6); no 131 to 210 MB scene downloads; the
stations within the limits either way. An explicit `?scene=` names the
first level's scene only, as the page's `sceneUrlOf` does, so a level
switch clears it. The tools follow: `rust-web-stations.mjs` builds by
default (`--query world=export` for the exports), and `level-switch`
runs built and `world=export`.

## D780. Gamepads on the web: the browser's Gamepad API, read directly

2026-10-04, WP 6.4. `play::gamepad` ports `Gamepad.js` (`Pads`, the
bindings, labels, the default map, the mute set, capture, per-pad maps,
rumble) over a plain snapshot of the pads (`Pad`: id, index, mapping,
axes, buttons), and `play::gamepad_io` reads the platform's pads into it.
On the web that is `navigator.getGamepads()` read property by property
through `js_sys::Reflect`, as `Gamepad.js` reads it: the `standard`
mapping's button and axis indices, the browser's `id` (the key of a pad's
map in `mr.padMaps`, so a map made in the JS game is the Rust build's
too, same origin) and `mapping` (a non-standard pad gets numbered labels
and resting-axis triggers). Bevy's gilrs backend was not used there:
gilrs on wasm re-maps the pad through its own layout (the raw indices a
non-standard pad's map stores are lost, and so is the id string), has no
force feedback on wasm (SPEC 8.2's rumble note), and would add to the
wasm. Reading through `Reflect` also reads a page's own `getGamepads`
(the tests' fake pad, as the JS suite installs it). Rumble is
`vibrationActuator.playEffect('dual-rumble', {duration: 140,
strongMagnitude, weakMagnitude})`, else `hapticActuators[0].pulse`, with
`reset()` to stop, each promise's rejection swallowed (`.catch(() =>
{})`), on the pad object of the same frame's poll. `window.__mp.pads` is
the state for the tests (`window.__pads`): connected, value, held, nav,
steerAxis, active, capture, rumbleOn, resetLabel.

## D781. Gamepads natively: gilrs's raw events as the standard mapping

2026-10-04, WP 6.4. Natively the client turns on `bevy_gilrs` (the
native feature table only) and adds `GilrsPlugin` where `DefaultPlugins`
has it. `gamepad_io` keeps its own pads from Bevy's `RawGamepadEvent`s,
not Bevy's `Gamepad` state, whose `GamepadSettings` add a 0.05 axis dead
zone and 0.75/0.65 press/release thresholds the JS does not have. They
are laid out as the standard mapping: buttons 0–16 (South, East, West,
North, the bumpers, LT and RT with their analogue values, Select, Start,
the stick presses, the D-pad, Mode), axes 0–3 with y negated (gilrs is
up-positive, the Gamepad API down-positive). `pressed` is value above
30/255, Chrome's threshold for an analogue button; since `bindingValue`
takes `max(value, pressed ? 1 : 0)`, a trigger pulled past 12 % reads
as full throttle or brake, which is what the JS game does in Chrome too
(its standard mapping marks a trigger pressed at the same threshold). A pad's index is the
lowest free one, as a browser hands them out; its id is written as
Chrome writes it, `Name (STANDARD GAMEPAD Vendor: 045e Product: 028e)`,
so the Controller screen shows the same name (the store is a file
natively, D572, so the maps are not shared with the web anyway). Rumble
is Bevy's `GamepadRumbleRequest`: `Stop` then `Add` for 140 ms, so a new
effect replaces the last as `playEffect` does; `reset` is `Stop`. Bevy
0.19.1's gilrs rumble uploads the weak motor's share as a second *strong*
effect with no replay length (`bevy_gilrs::rumble::get_base_effects`), so
natively the weak magnitude also drives the strong motor; left as Bevy
has it, for the owner to judge with a real pad (a virtual uinput pad
received only strong-motor effects). gilrs also lists any HID device with
a joystick interface: on this machine a Hall-effect keyboard shows up as
pad 0 (no SDL mapping) and is the pad in hand until a real one is
touched; Chrome lists such devices too. `RUST_LOG=
mp_game::play::gamepad_io=debug` logs what the pads ask for each time it
changes, for trying a controller.

## D782. When the pads are polled, and how the race reads them

2026-10-04, WP 6.4. `main.js`'s `tick` polls the pads first
(`input.update` → `pads.poll`), then the Controller screen, the menus'
navigation and the race. The client polls once a frame in `PreUpdate`,
after Bevy's input systems, so every screen and the race see the same
poll; `now` is real time in ms. The race's input layer runs at the tick
rate (D433), so it takes the frame's poll once (`Input::pad_frame`, in
the race's frame before the ticks): the one-shot presses (camera,
reset, pause) are posted then, once, and each tick's `Input::update`
reads the rest (`value`, `held`, `digital`, `steerAxis`) as `Input.js`
does: the stick past 0.12 is `sign·((|ax|−0.12)/0.88)^1.4` through the
kernel's `pow` and analogue, steering bound to buttons ramps like keys,
the triggers merge by max above 0.05, nitro, handbrake and look back OR
in. Posting the presses per tick would fire a reset twice in a frame of
two ticks. What the race asks of the pads in a frame (`kick`s, `feel`,
`hush`) is handed to them at the next frame's poll, before it reads the
pads and flushes the rumble: a kick's start is then the previous poll's
time, as `pads.now` was when `Race.update` kicked, and the flush is at
the next poll, as in the JS. The kicks are `Race.js`'s and
`PursuitView.js`'s: countdown beeps, GO, shifts, landings, wall and car
hits (`jolt`), the nitro firing (looked at per tick, where the JS looked
once a frame), and the player's takedown, spikes, barrier and bust; the
steady buzz (`Race.rumble`: scrape, gravel, skid, nitro, spiked tyres)
from the state after the frame's ticks, handed on only while a pad is
connected, and not while paused, so it lapses after 150 ms. A race
built, restarted or resumed hushes the pads (`startRace`'s and
`pause(false)`'s `pads.hush()`), at the next poll with the last poll's
state, which is the state the JS's `hush` reads. The Rumble switch is
followed from the menus' settings (`rumbleOn`, and the 0.5/0.7/300 ms
buzz when it is switched), the maps load from `mr.padMaps` and are saved
there when a capture binds or Defaults resets. The stuck hint names the
pad's reset button while one is connected (`PRESS BACK TO RESET`).

## D783. The menus with a gamepad

2026-10-04, WP 6.4. `ui::nav` ports `MenuNav.js` over the screens'
controls: a control's box is its laid-out rectangle in CSS px (what
`getBoundingClientRect` gave), the focusable ones are the screen's
controls that take an action (the open drop-down's list left out), and
the highlight is the screens' focus ring (`UiState::focus`, which Tab
moves too, D573), drawn in #ffcf4d while a slider or drop-down is being
adjusted (`.pad-edit`). The repeat timing (380 ms, then every 110 ms),
the scoring (along + 3 × off-axis), the first push only showing where you
are, the per-screen memory, A to press or to adjust, ◂ ▸ by five steps of
a slider or one option of a select (its `change`: `Act::Choose`), B and
Start per screen (`main.js`: B resumes from pause, goes to the menu from
the results and closes the Controller screen; Start races from the menu
and races again from the results), and the mouse or a finger hiding the
highlight are the JS's. What the DOM gave by markup is named per screen:
the first control of a screen is the Controller screen's first row
(`data-nav-first`), else the primary button (Race, Resume, Race again).
`scrollIntoView({block: 'nearest'})` scrolls only a control not wholly in
view, to the middle (the `reveal` the screens already had), once the
screen rebuilt with the highlight is laid out; `after_layout` now leaves a
screen rebuilt that frame alone (it has no layout yet, and clamping its
offset to an empty content had sent every rebuilt screen back to the
top). A new
screen's highlight waits for that screen's controls to be built (a frame
later than the DOM, which had them all along). `body.pad` is
`UiState::pads`, set from the pads each frame, which shows Controller
setup on the menu and Controller on the pause screen.

## D784. The Controller screen

2026-10-04, WP 6.4. `ui::pad_setup` ports `PadSetup.js`: the pad's
name (its id less the browser's "(STANDARD GAMEPAD Vendor: …)", with
"· remapped" or the not-standard warning), each action's bindings as
labels joined with " / ", rows lit while held (`.on`, not while
listening), the listening row in gold, the hints, a row picked to listen
(again to stop), Defaults and Done, and the Rumble switch (saved, D782).
The screen (`screens::padsetup`) is rebuilt when what it shows changes.
Esc, P and Start leave it or stop listening, before the race can take
them as un-pause (`main.js`'s `padSetup.escape()` on `consume('pause')`):
with a race (opened from pause) it consumes the race's pending pause in
the race's frame, after the pads reach the input layer and before the
ticks; from the menu (no race) it reads Esc and P itself and the pad's
Start press. The Controller screen left any other way stops listening.
`window.__mp.padsetup` (`name`, `binds`, `on`, `listening`, `hint`) is the
screen for the tests, which read its DOM in the JS suite.

## D760. The race's headlight spot, and three's spot lights as a list

2026-10-04, WP 4.4 (the headlight spot; D440 left it open), on the owner's
report that night races in the Rust build were too dark (the start of
Coast Highway, the end of Sierra). `Race.js` gives the player's car "one
real spotlight": `new THREE.SpotLight(0xfff2d6, 0, 140, 0.55, 0.55, 1.2)`,
no shadow, on `pModel.headlightAnchor` at its origin, its target at
(0, -2.2, 30) in the anchor's frame, `intensity = lightsOn × 140` each
frame (`lightsOn = smoothstep(0.25, 0.6, night)`), removed by
`dispose()`. Ported as `play::headlight`: `draw` sets the intensity, and a
`PostUpdate` system after the transforms propagate (three reads
`matrixWorld` when it renders) and before the globals are packed places
the spot from the anchor entity's `GlobalTransform` (WP 4.1's
`headlight_anchor`, which `play::models` now keeps per car). Without a race
(`race.dispose()`, the menu) it is gone; a restart keeps the cars and so
the spot, as the JS's new `Race` makes a new one.

`Lighting` holds it as `headlight`, beside the scene's `spot` (the desert
train's beam, which follows its animator, D720/D721). three keeps every
spot light of the scene in `spotLights[]`, in render-list order (the
world's lights before the race group's; neither casts a shadow, so the
sort that puts shadow casters first keeps that order), and the shader
loops over `NUM_SPOT_LIGHTS`. The globals carry two spot slots: the first
at texels 19 to 22 as before, the second at 27 to 30 (`G_SPOT1_POS`), and
`pack` fills them in three's order with no gap (the headlight is slot 0
where the level has no spot). The material shader loops over the slots and
stops at the first empty one. A light at intensity 0 stays in the loop
(the train's beam by day, the headlight before dusk), as in three, where it
costs the same and adds nothing. Physically correct units as before:
colour × intensity, distance cut-off, decay, cone and penumbra cosines.

## D761. The headlight checked against the JS in races at night

2026-10-04. The same race on both sides (seed 1, the sports car, the
autopilot, pursuit off), stopped at the same race time: the JS with
`?parity=1&ticks=16`, stopped exactly at the tick through `__parity.onTick`
and pictured three times from the frozen frame (as drawn; with WP 4.4's
effects hidden, which are not ported: the fake headlight pools, smoke,
sparks, skids; and then also with the spot at 0); the Rust web build
through the registered server, the page itself asking for the screenshot
the first frame the race time reaches it (tick for tick with the JS). The
scripts and pictures are in `parity/report/headlight/` (not in git).
Mean linear luminance (Rec. 709) of boxes on the road ahead:

| Where | JS no spot | JS spot | Rust before | Rust after |
|---|---|---|---|---|
| Coast 20 s, desktop hq, beside the rival ahead (left, right) | 0.0026, 0.0012 | 0.0038, 0.0023 | 0.0026, 0.0013 | 0.0040, 0.0024 |
| Coast 20 s, desktop hq, the lit cliff | 0.0057 | 0.0181 | 0.0058 | 0.0187 |
| Coast 20 s, iPhone portrait hq off, road ahead; car ahead | 0.0156; 0.0140 | 0.0194; 0.2615 | 0.0166; 0.0169 | 0.0203; 0.2683 |
| Sierra 170 s (city freeway), desktop hq, band round the rival | 0.0022 | 0.0038 | 0.0021 | 0.0038 |
| Sierra 170 s, iPhone portrait hq off, band round the rival | 0.0040 | 0.0060 | 0.0058 | 0.0082 |
| Desert 110 s (dry lake, train beam in the scene), desktop | 0.0339 | 0.0594 | | 0.0608 |

The spot matches: the pool's shape and place, the rivals and the
roadside it lights. The WebGL2 build gives the same (Coast desktop
0.0041, 0.0025, 0.0188; phone 0.0204), and the native client draws the
same pool. Frame cost (Coast at 15 to 20 s, 1280 × 800, uncapped, WebGPU,
four alternating runs each on a loaded machine): median 3.35 ms with the
spot, 3.13 ms without, within the runs' spread (2.9 to 4.0 ms).

What still differs on the road ahead is not the spot: drawn as the player
sees it, the JS road round the car ahead is several times brighter again
(Coast 0.0164 and 0.0138 in the boxes above, Sierra 0.0194, against 0.0038
without the effects) because of `Effects.js`'s fake headlight pools (an
additive glow quad on the road ahead of each car, 9 × 16 m for the player
and 7 × 12 m for the others, its opacity rising with night), WP 4.4's
effects, still to port.
Those, not the spot, are most of the "too dark" the owner sees.

## Menu section decisions

D676's package: the menu switches levels at once, over a short section of
each level built in the client (`crates/mp_game/src/preview.rs`,
`crates/mp_worldgen/src/section.rs`). The deviation is in DEVIATIONS.md.

## D740. The menu flies over sections, prepared for every level

2026-10-04 (D676). A menu-first run (`ui::menu_first`, not a test scene,
station run, fly camera or given scene file; `?sections=0` keeps the old
boot) downloads no level. The client builds a **section** of every level
with `mp_worldgen` (D741), spawns each once under a root entity of its own
(hidden unless shown) and shows the selected one behind the menu. A level
tab or card only selects (it saves `mr.level` and plays the level's music,
as before): if that level's section is up it is shown in the same frame,
else as soon as it is. Showing one swaps what the client's single-scene
systems read (D742), so the attract camera, sky and time of day, fog,
environment map, local lights and the scenery's animators run on the
section as on the level. Race frees the sections and builds or downloads
the level whole behind the loading screen (D743). Generated in the client
rather than cut from the exports: nothing to download (the exports are 39
to 210 MB, D439), and the section is the level's own build, so it draws
what the level draws there.

## D741. What a section is: the level's build near the attract camera's run

2026-10-04. The attract camera starts at `startS + 60` (`toMenu`) and
flies **500 m** (31 s at 16 m/s), then starts over (`preview::wrap_attract`;
the JS's runs the whole first zone, 2 to 14 km, which a section does not
have). The stretch built is `startS + 20` to `startS + 660` (the eye is
22 m behind, the camera looks 20 m ahead, and 100 m of road beyond the
run's end). `mp_worldgen::section::Section` restricts a level's build,
with `World::section` set (a new field, `None` for a level, which changes
nothing):
- **Terrain**: only tiles within **2,500 m** of the stretch are meshed
  (`section::keep_tiles` in the terrain's mesh stage), at the steps and
  seams the whole grid gives them; the far ridges stay, the rest of the
  route's land is not built.
- **Scenery**: every module's `plan()` runs (its flattens and carves shape
  the land everywhere, so heights are the level's), but its `build()` only
  when one of the zones naming it passes within 600 m of the stretch
  (`section::builds_module`): Sierra builds Mountain only.
- **The cut**: after the last job, every drawable whose bounds (an
  InstancedMesh's: all its drawn instances') lie wholly beyond **600 m**
  of the stretch leaves the tree, and geometry nothing drawn uses is
  dropped before the scene is assembled (`section::cut`,
  `section::finish`). Animators addressing what was cut find nothing
  (`WorldBuild::resolve` already drops such edits).

Seeds and the order of draws are the level's, so what is kept is what the
level draws: natively, 200 m into the run, the section and the whole
level (`?world=gen` for Sierra, the exports for the others; the other
sections built with all of `mp_worldgen`'s scenery for the comparison,
`MP_SECTION_SCENERY=1`) give the same pictures for all six levels, the
only differences beyond 600 m (a far hill on Seaside, a distant lorry on
Desert). The radii and length are `preview::{CAMERA_RUN, TERRAIN_RADIUS,
SCENERY_RADIUS}`; at 600 m the city skylines the sections look at stay
(Cruise's towers are within it). `cargo run --release -p mp_worldgen
--example section_cost -- <levels>` (`LEN=640 RS=600 RT=2500` for a
section, `JOBS=1` per job) times each job and counts the heap. Natively
(load 7): sections build in 0.25 s (Seaside) to 1.1 s (Streets, Cruise),
the whole levels in 0.25 to 1.6 s.

## D742. How sections are held and shown

2026-10-04. `preview::Previews` holds one entry per level, built one at a
time: natively on a thread, on the web a few jobs a frame (30 ms while the
loading screen is up, 8 ms behind the menu), then spawned by the loader a
slice a frame (`loader::step_section`, `loader::finish_section`: the level
path's code, with every entity under the section's root, `Build::under`).
Its CPU copy goes as a level's does; what stays is its world (animators,
sky), its animators' index (`animate::SceneIndex`, entities set per node
from the root's children), its Track, lights and sky noise. Showing one
(`preview::show`, an exclusive system, under 0.4 ms) hides the root shown
before, makes the new one visible, and puts its Track and level in
`TrackRes`, a new `SkyState` in `SkyRes` (the environment map rebuilds),
its spot and point light in `Lighting`, its noise in `EnvRequest`, its
world in `animate::WorldGen` (`install_world`) and its index as the
`SceneIndex` resource, clears the animation blocks (material numbers are
per scene), sets `opts.o.level` and the attract camera. Nothing else
changes; `animate::wants_world` is false while sections are drawn, so no
level world is built for animators behind them. The first section goes
through `AppState::Running` and the warm-up (D390) behind the loading
screen; later ones get their own stand-ins (`SectionWarm`), despawned once
nothing compiles, so a tab never compiles a pipeline (no frame over 50 ms
in the 60 after each switch). The page skips its boot download when
`menu_sections()` says so and fetches Seaside's survey (0.6 MB) for its
section (`section_survey`). Order: the saved level's first; the others
once the menu is ready (not during its warm-up), the largest build peaks
first (Streets, Cruise, Desert, Coast, Sierra, Seaside: D745), a tapped
tab's level moving to the front. Natively `--query
sectionshots=<dir>[&shot_s=200&menu=0]` saves each level's section and
exits; `menu=0` draws no screens.

## D743. Race, Main menu and the level drawn whole

2026-10-04. Race (`ui` Act::Start) frees the sections (roots despawned,
worlds and indexes dropped, any build in flight abandoned) and, unless
that level is drawn whole already, asks for it as a tab did before
(`__mp.reload` on the web, a thread natively): downloaded, or built by the
client with `?world=gen`, behind the loading screen; the countdown arms
once the level drawn whole is that level (`Previews::full`) and ready.
Main menu after a race keeps the raced level drawn whole for its tab, as
the JS's `toMenu` does (attract camera at `startS + 60`, its own run), and
prepares the six sections again behind the menu, that level's last. The
first time another level is shown the whole level is freed (its entities
despawned, its world dropped); Race on the level still drawn whole starts
at once, as Race again does. `unload_scene` despawns with `try_despawn`
(a section's entities go with their root). The level-switch suite
(`tools/parity/e2e/level-switch.test.mjs`) now checks that a tab loads no
scene and Race loads one; `tools/parity/e2e/sections.test.mjs` checks the
whole flow and prints the measurements of D745.

## D744. Levels the client does not build yet: terrain, road, sky and sea

2026-10-04. A section of a level that `animate::generated` does not name
is built from the same stages with no scenery (terrain, road, sky, sea;
Seaside with its survey and ground colours), the modules' `plan()` not
run: a stand-in until that level's client build lands. On this branch
that is every level but Sierra; the agents adding Seaside and Cruise
(D680+), Coast (D700+) and Desert and Streets (D720+) make
`animate::generated` true for theirs, and their sections then come from
`animate::section_build` (their `new_build`, with the section set). When
merging: `section_build` calls `new_build(level)`; with D680's change it
becomes `new_build(level, true)`, and Seaside's survey and photo come from
`levels::seaside`'s stash (the page then fetches the photo at boot in a
sections run too, `loadPhoto`, and `preview::level_ready` should wait on
`levels::inputs_ready(level, true)` as well as its own survey). A section
builds its terrain tiles three to a job (`terrain_mesh`, sections only),
so behind the menu no tile job holds a frame long.

## D745. Measurements

2026-10-04, release web build, headless Chrome on this machine's RTX 3060
(1280 × 800), `tools/parity/e2e/sections.test.mjs`; the machine shared
(load averages given; other runs holding GPU memory).
- **Wasm size**: 8.12 MB (WebGPU) and 8.50 MB (WebGL2) after gzip, +0.03
  MB over b367862 (D675's 8.09 / 8.47).
- **Time to the menu** (navigation to the menu up, every pipeline
  compiled): 4.59 and 4.65 s at load 1.8 to 8.5 (first frame 1.9 s, the
  Sierra section built in 1.3 s and spawned in 0.1 s); the JS game reaches
  its menu in 3.2 s here (BASELINE), so within SPEC 6.6's 5 s; nothing is
  downloaded but the client and the survey. All six sections are up 4.8 s
  after the first began.
- **Tab switch**: the section shows in the frame after the click (the
  harness's click itself waits two frames: 78 to 84 ms from the click to
  the report); natively `show` takes 0.1 to 0.3 ms. No frame over 50 ms
  in the 60 frames after any switch.
- **Memory with all six held** (wasm high-water): 202 to 217 MB with
  this branch's sections (Sierra's whole, the others terrain and road),
  against 488 MB for the boot that downloaded Sierra's export (D492). With
  every level's scenery (a measurement build linking all of
  `mp_worldgen`'s scenery, what the client will hold once every level is
  generated): 160 MB at the menu, **508 to 565 MB** once all six are up
  (load 7 to 21). That is over SPEC 6.6's 512 MB, and it comes from one
  build, not from what is held: Streets' downtown module peaks at 448 MB of
  heap while it builds (23 MB in use before it, 164 MB after; the whole
  level's build peaks at 451 MB), Cruise's city at 278 MB; the other
  sections peak at 28 to 112 MB (`section_cost`, natively). The sections
  held cost little (the GPU holds their meshes and images), and the
  heaviest are built right after the first, while little else is held. A
  race on a client-built Streets reaches the same peak.
- **Frames behind the menu** while the other five build: with this
  branch's sections p50 16.7 ms, 11 frames over 50 ms, the longest 100
  ms; with every level's scenery, 17 to 43 frames over 50 ms, the longest
  1.2 s (2.2 s at load 21): a scenery module's `build()` is one job
  (Streets' downtown, Cruise's city, Desert's, 1 to 1.5 s each in wasm
  here).
- Race from the menu (Coast, its terrain-road-sky export through the
  harness): racing 1.5 s after the click; then Main menu, the sections
  again, Coast freed for Desert's section, and a race on Seaside.
- **WebGL2**, the same suite at load 140 (timings not comparable): passes;
  wasm 155 MB at the menu, 213 MB with all six held, 290 MB after the two
  races; tabs show within 115 to 362 ms of the click under that load.
  Also passing: `level-switch.test.mjs` (both forms, WebGPU, load 165) and
  the phone menu-to-race check (`phone-menu.mjs`, iPhone emulation: a tap
  on Seaside's tab, Race, racing with the throttle, the sound running).

## D746. Open, for the owner

2026-10-04. (1) **Memory**: with every level generated, the menu's
high-water reaches about 510 to 565 MB because Streets' world build
peaks at about 450 MB of heap (D745); a Streets race would too. Either
the Streets module's build gets leaner (its agent is told), or the menu
builds Streets' section only when its tab is first chosen (that tab then
waits about 2 s on a desktop, longer on a phone), or the budget is
looked at again. Not decided here; the sections stay eager. (2)
**Hitches behind the menu**: a scenery module's build cannot be split, so
the first seconds of the menu stutter while the others build (frames up
to 1.2 s here, more on a phone); the alternatives are building them all
behind the loading screen (about 11 s here instead of under 5, against
the JS's 3.2 s) or a worker (a second wasm instance and its memory). (3)
The camera's 500 m run and the radii (D741) are a first choice; phone
checks of the time to the menu and of memory are owed.

## D747. With every level built in the client (main 01e660c)

2026-10-04, after merging main, where `animate::generated` names all six
levels. D744's notes are done: `section_build` calls `new_build(level,
true)`; the menu's survey also goes to `levels::seaside`'s stash, a
section waits on `levels::inputs_ready(level, true)`, and a sections run's
page fetches Seaside's photo at boot (kept, shared with the race's build).
D744's terrain-and-road stand-in is now only a fallback. Natively the six
sections at 200 m into the run match the whole levels as in D741, Seaside
now with its draped photo. Measured with `sections.test.mjs` on the real
builds (release, 1280 × 800; the machine heavily shared, load averages
given):

| | WebGPU, load 20 | WebGPU, load 95 | WebGL2, load 65 |
|---|---|---|---|
| Menu ready (nothing downloaded) | 5.01 s | 5.99 s | 5.68 s |
| First section (Sierra) built, spawned | 1.54, 0.12 s | 1.44, 0.12 s | 1.88, 0.13 s |
| Wasm at the menu | 176 MB | 176 MB | 169 MB |
| All six up after the first began | 12.8 s | 14.0 s | 17.0 s |
| Wasm high-water, all six held | 553 MB | 537 MB | 496 MB |
| Frames behind the menu over 50 ms (of 157); longest | 35; 1.32 s | 31; 1.28 s | 41; 1.65 s |
| Tab shown after the click (harness's two frames included) | 81 to 131 ms | 81 to 131 ms | 77 to 115 ms |
| Frames over 50 ms in the 60 after a switch | 0 | 0 | 0 |

Section builds in wasm here: Seaside 0.4 s, Coast 1.3, Sierra 1.5, Streets
1.7, Desert and Cruise 1.9 (spawns 0.12 to 0.5 s). The memory is D745's
finding on real builds: over SPEC 6.6's 512 MB on WebGPU because of
Streets' build peak, not what the sections hold; the long frames are the
scenery modules' one-job builds. Both stay the owner's (D746); nothing
chosen here. `level-switch.test.mjs` (both forms) passes on WebGPU and on
WebGL2 (`MP_BACKEND=webgl2`, new), as does `sections.test.mjs` on both.
Wasm 8.42 MB (WebGPU) and 8.80 MB (WebGL2) after gzip with every level's
world generation linked.

With D678 merged (main 7bcdb89: the client builds every level by default),
Race from the menu builds the level in the client, nothing downloaded.
The same suite then (load 45 to 49, so the times are long):

| | WebGPU | WebGL2 |
|---|---|---|
| Menu ready | 6.11 s | 5.16 s |
| Wasm at the menu; all six held | 176; 505 MB | 169; 503 MB |
| Frames behind the menu over 50 ms (of 165 to 173); longest | 38; 1.67 s | 48; 1.92 s |
| Tab shown after the click | 76 to 132 ms | 105 to 171 ms |
| Race on Coast: racing after the click; wasm high-water | 6.4 s; 596 MB | 5.6 s; 609 MB |

A Coast build alone reloads at 512 MB (D678); after the menu's sections
the race reaches 596 to 609 MB, the sections' peak and the race's build
not sharing all their memory (the wasm memory never shrinks, D745). Part
of D746 (1). `level-switch.test.mjs` (built and `world=export`) passes on
both backends.

## D748. The menu shows simplified views, not cut-down levels

2026-10-04, the owner on D746: "let's build simplified views for the
menu. loading every level for the menu is not really what i intended";
the stutter behind the menu is accepted for now. A level's menu **view**
replaces D741's section of its full build:
- **The level's land, road, sky, time of day and sea** from its own world
  build, with `Section::scenery` false: every scenery module's `plan()`
  runs (so the land and road are the level's), none builds. Terrain tiles
  within **1,500 m** of the stretch (2,500 before; the radius changes
  little: the textures dominate), the road cut at 600 m as before, and the
  sea over the view's land only (`Sea::new_in` / `sea_geometry_in` with
  `section::bounds`; a level's sea is unchanged): Coast's view 29 → 16 MB
  of scene data.
- **Stand-ins for its scenery** (`preview/hints.rs`, a job added after
  the build with `Build::push_job`), chosen from what the attract camera
  sees on each level and built from `mp_worldgen`'s flora templates and
  boxes, with a fixed seed per level: Sierra a few spruce and fir up the
  slopes and boulders by the road; Coast telegraph poles on the inland side
  and scrub; Streets blocks of tinted walls with a generated window texture
  lit after dark (`add_night`), lit shopfronts and street lamps; Desert
  red hoodoos (stretched rocks under a cap) and scrub on the canyon floor;
  Seaside the red, white and blue barrier, catch-fence posts and oaks on
  the hills; the Cruise a skyline of lit towers and freeway lamps. These
  are invented, not the JS's scenery (DEVIATIONS.md).
- **Seaside's photo**: on the web the view drapes a quarter-size copy
  (461 × 540, 1 MB) that the page scales on a canvas and hands in
  (`section_photo`); the race's build fetches the whole photo as before.
  Natively the view uses the level's own setup and photo.

Everything else is D742 and D743: one view per level, built when the menu
opens (the saved level's first), held under hidden roots, shown in the
frame after a tab; Race frees them and builds the level whole (D678's
default); Main menu keeps the raced level for its tab. The heaviest-first
order (D745) is gone (views are light); they are built in menu order after
the selected one. Pictures of every view beside its whole level from the
same camera: `parity/report/menu-views/index.html` (natively, 200 m into
the camera's run; `--query sectionshots=<dir>&menu=0` takes the views).

## D749. What the views cost

2026-10-04, release web build, headless Chrome, RTX 3060; load averages
14 to 25 unless said.
- **Memory at the menu** (wasm high-water): 81 MB before the first view is
  built; 137 to 151 MB once Sierra's view and the menu's warm-up field are
  up; **169 to 172 MB with all six views** (WebGPU and WebGL2), so the six
  views and the warm-up field add about 91 MB, against 505 to 553 MB for
  D747's cut-down sections. Counted with a heap-counting allocator (a
  diagnostic build, not committed): the five views after the first hold
  about 13 MB of heap between them (96 → 109 MB in use); each peaks about
  15 to 35 MB above that while it builds.
- **Race after the menu** (Coast, built): wasm high-water 515 to 564 MB on WebGPU,
  516 to 564 MB on WebGL2 (most runs 548 to 564; they vary run to
  run), against Coast raced
  straight from the address here: 466 to 498 MB (WebGPU), 473 to 507 MB
  (WebGL2), and 512 MB on reload (D678). The heap in use when Race is
  tapped is about 98 MB after the views are freed (11 MB go with them),
  against about 50 MB when a race from the address starts its build: the
  menu, its warm-up field and the sound are live during the build, and the
  build's own peak (about 410 MB of heap) lands on top. So the race peak
  is about 10 to 100 MB above the level's own (most runs 50 to 90),
  from the menu rather than
  from the views. Not met strictly; raised (D750).
- **Time to the menu**: 3.75 to 4.77 s (WebGPU, load 23 to 34), 3.74 to 4.40 s (WebGL2,
  load 14 to 20); the JS's is 3.2 s here. Views build in 0.3 to 1.3 s each
  in wasm (Sierra's first, 1.3 s); all six up 5.8 to 7.3 s after the first
  began.
- **Frames behind the menu** while the other five build: 16 to 19 of
  about 90 over 50 ms, the longest 200 ms (D747: up to 1.9 s).
- **Tab switch**: shown the frame after the click (77 to 182 ms including
  the harness's two frames), no frame over 50 ms after any switch.
- **Wasm size**: 8.43 MB (WebGPU), 8.81 MB (WebGL2) after gzip (+0.01 MB
  for the stand-ins).
`sections.test.mjs` and `level-switch.test.mjs` pass on both backends.

## D750. Open: the race after the menu

2026-10-04. A race started from the menu reaches 515 to 564 MB of wasm
memory where the same level raced from the address reaches 466 to 507
(D749), because what the menu holds (its warm-up field, the sound, the
screens; the views are freed) is live while the level builds. Ways to close
it, none taken here: drop the menu's warm-up field earlier or not build it
(D574 made it to warm the cars' pipelines), build the level before the
race's field and sound come up, or accept the menu's share. For the owner.

## D800. `Effects.js` in the client: its state, and what draws it

2026-10-04, WP 4.4 (the rest of it after D760). `Effects.js` is ported in
two halves. `play::effects` is its state without the engine, in the JS's
structure and names: `Particles` (the 700-smoke and 500-spark ring
buffers, simulated on the CPU, every array an `f32` store as the JS's
`Float32Array`s: drag, gravity, growth, the alpha's fade in over the
first tenth of the life and out over the rest), `SkidMarks` (the
2,400-quad ring, a quad across the travel 3 cm above the road, segments
under 5 cm or over 4 m dropped), and per car (`addCar`, in the JS's order:
the player, the rivals, the traffic pool) its flames' lengths, its last
rear-wheel contacts and its headlight pool, with `sparksAt`, `smokeAt`,
`wheelWorld`, `resize` and `update` as the JS has them; the inexact math
through `mp_math::kernel`. `play::fx` draws it (D803), and `play`'s
`effects_frame` runs it once a rendered frame after the cars are placed,
as `Race.update` runs `effects.update(dt, night, this.extras)` after its
visual sync: the extras are the player's `{ nitro, skid, launch }` (launch:
counting down with the throttle over half, the throttle the last tick
read) and each rival's `{ nitro }`; traffic has none. The cars are taken
between the last two ticks, as they are drawn (SPEC 6.5). Before the
update come the frame's sparks, in `Race.update`'s order: a car-to-car hit
with the player (`round(8 + strength × 30)`), the player's wall impacts
(`round(6 + strength × 40)`), from the frame's tick events, then the
scrape's two sparks at the car's side. A paused or held race does not
update its effects (the JS does not call `race.update`); a restart makes
them new (the JS's new `Race` builds new `Effects`); `race.dispose()`
takes them away, the flames with the cars. `?fx=0` races without them (a
measurement switch).

The JS quirk SPEC 13 names is kept: every pool draws with the one shared
`poolMat`, whose opacity the last line of `update` sets to 0.3 × night, so
the per-car `0.35 × night` is a dead store and every pool, the player's
too, shows at 0.3 × night. `tailPoolMat` is made and never used; it is
not ported. The police's pools and the pursuit's smoke and sparks
(`PursuitView`'s calls into `Effects`) are M8's.

## D801. The effects' randomness: their own stream, in the JS's order

2026-10-04. The JS draws every effect's randomness from `Math.random`,
which nothing else in a race shares any more (the simulation has its
streams, SPEC 4.3). The client's effects own a `mulberry32`, seeded from
the race's seed (`seed ^ 0x5eed0e44`), so a seeded race's effects are the
same each time, and draw from it in the JS's order exactly: per car the
flames' lengths, the skid smoke's chance, its wheel, then `smokeAt`'s four
draws (vx, vy, vz, life); per spark `sparksAt`'s six (vx, vy, vz, life,
size, green). The staged scenes (D804) use `mulberry32(scene seed)` on
both sides, so the JS and the Rust emit the same particles to the last
bit: the sparks of `sparks-flames` land on the same pixels.

## D802. What the JS does once a frame, at its 60 Hz rate

2026-10-04, SPEC 6.5. A chance the JS takes once a frame becomes the same
chance per 1/60 s at any frame rate: `1 − (1 − p)^(60 dt)` (exactly p at
60 frames a second), for the skid smoke (`p = skid × 0.55`) and the
scrape's sparks (`p = 0.6`); the launch puffs, one a frame in the JS,
are one per 1/60 s (a remainder carried). Still once a frame, as the JS:
the flames' random lengths (a flicker, not an amount), and the skid
quads (one per wheel per frame; at 120 frames a second they are half as
long and the 2,400-quad ring holds half the distance, and a slow skid's
segments may fall under the 5 cm floor, but no mark is ever seen, D806).
The particles integrate over the frame's dt as the JS's.

## D803. The effects drawn: quads in the slab, two shader kinds, a light slot

2026-10-04. `play::fx`:

- **Smoke and sparks** are one mesh each of quads, four vertices a point
  (D294), at the origin and never culled (`frustumCulled = false`); the
  JS's `Points` sort by their origin too. The `Particles` kind is
  `Patch::Particles` in `three_material.wgsl`: `gl_PointSize = aSize ×
  uScale / max(0.1, −z)` (clamped to the GL range), the texture (the smoke
  texture, normal blending; the glow texture, additive) tinted by the
  point's colour, alpha × the point's alpha, then three's plain fog (a
  `ShaderMaterial` has no lights, so no sun tint). `uScale` is the
  material's, set by `resize` from the drawing buffer's height and the
  camera's field of view when the race starts and when the window's
  height changes (`main.js`), not as the field of view widens with speed.
  The vertices go each frame a particle lives into the mesh's place in
  Bevy's vertex slab (`animate::MeshWrites::push`, D701), never into the
  asset; a ring with nothing alive is not written.
- **Skid marks**: the 2,400-quad mesh, written when a mark was added,
  `Patch::Skid` (`vec4(0.02, 0.02, 0.02, vA)`, normal blending, front faces
  only as three's default side, polygon offset (−4, −4)).
- **Flames**: the JS's open cone (`ConeGeometry(0.13, 1, 10, 1, true)`
  turned to point down −Z, moved back half its length) built by
  `mp_worldgen`'s three geometry, two `MeshBasicMaterial`s (0x66aaff at
  0.85 and white at 0.9, additive, no depth write), the outer at each
  exhaust on the car's body entity, the core inside it scaled (0.45, 0.45,
  0.6); shown with the nitro, `scale.z` the frame's random length.
- **Pools**: the JS's `PlaneGeometry(1, 1)` laid flat with the glow map,
  0xfff1d0, additive, no depth write, one entity per car scaled 9 × 16 m
  (7 × 12 for the others), placed 10 m (7.5 m) ahead along `yaw +
  visualYaw`, 6 cm above the car. The shared opacity sits in a light slot
  of the globals (`MaterialLights`, D456): a plain material with 3 in
  `night.z` takes its opacity from slot `night.w`, so no material is
  edited per frame.
- **Polygon offset with its slope.** The pools first drew with a constant
  depth bias only (D103's conversion): where the road is crowned or
  banked under a flat pool, the road's own depth beat it and half a pool,
  or all of it, vanished (the `skid-smoke` and `sparks-flames` scenes).
  three's `polygonOffset(factor, units)` grows with the polygon's depth
  slope; the effects' materials now carry it as a slope-scaled bias too
  (`ThreeKey::depth_slope`, −factor: 6 for the pools, 4 for the skids),
  beside the constant. Other materials keep D103's constant alone; one
  that loses to the road the same way can take the same field.
- Every effect material × mesh layout joins the warm-up with the cars
  (D458), so a race compiles nothing once it runs; none casts a shadow.

## D804. L4: the staged effect scenes

2026-10-04. `parity/golden/effects/scenes.json` holds five staged scenes
(pools at blue hour; a skid with its smoke at night; the same skid by
day; nitro flames and three bursts of sparks; launch puffs and smoke
bursts), each at a fly-camera station of its level, the scenery frozen:
cars on the road at s + speed × frame × dt, a body at the last pose for
the flames, the frames run at once (each frame's bursts, then
`update(dt, night, extras)`), `Math.random` = `mulberry32(seed)`.
`tools/parity/effects-scenes.mjs --side js` draws them with the JS's own
`Effects.js` in the game's page (kernel on, the export's seeded
`Math.random` restored after); `--side rust` loads the web build with
`?fx=<scene>` (`play::fx_stage`), which stages the same through
`play::fx` once the level is up and reports `__mp.fxStaged`. At 1280 ×
800 against the JS (`cargo xtask parity shots`), all within SPEC 12's
limits on WebGPU and on WebGL2 (the same numbers to the second decimal):

| Scene | mean ΔE00 | 95 % block |
|---|---|---|
| pools | 0.133 | 0.331 |
| skid-smoke | 0.966 | 5.675 |
| skid-day | 0.197 | 0.385 |
| sparks-flames | 0.139 | 0.382 |
| launch | 0.235 | 0.415 |

The particles, the flames and the pools land on the same pixels. What is
left in `skid-smoke` is the smoke against the bottom-left edge, a little
brighter in the JS: smoke points a few metres from the camera, hundreds
of pixels wide, whose centres fall just outside the view. The port drops
such a point as GL ES says (D294); Chrome's GL on Vulkan here appears to
draw some of them. Within the limits, and it is D294's rule for every
kind of point, so it is left.

## D805. The night races with the effects, against the JS as drawn

2026-10-04. D761's setup and pictures (seed 1, the sports car, the
autopilot, pursuit off; the JS stopped at the tick, the Rust page taking
its own screenshot the first frame the race time reaches it), the JS
frames as drawn and with the effects hidden, through
`tools/parity/race-night.mjs` and `tools/parity/lum.py`. Mean linear
luminance (Rec. 709) of boxes on the road round the car ahead:

| Where | JS no effects | JS as drawn | Rust before | Rust now | Rust WebGL2 |
|---|---|---|---|---|---|
| Coast 20 s, desktop hq, left and right of the car | 0.0066, 0.0051 | 0.0184, 0.0134 | 0.0066, 0.0051 | 0.0184, 0.0140 | 0.0183, 0.0144 |
| Coast 20 s, desktop hq, the whole pool | 0.0138 | 0.0171 | 0.0154 | 0.0186 | 0.0188 |
| Sierra 170 s, desktop hq, left and right of the car | 0.0052, 0.0038 | 0.0372, 0.0212 | 0.0059, 0.0044 | 0.0416, 0.0244 | 0.0400, 0.0234 |
| Sierra 170 s, desktop hq, the whole pool | 0.0155 | 0.0254 | 0.0162 | 0.0270 | 0.0267 |
| Sierra 170 s, iPhone portrait hq off, left and right of the car | 0.0112, 0.0052 | 0.0267, 0.0190 | 0.0168, 0.0060 | 0.0312, 0.0215 | |
| Coast 20 s, iPhone portrait hq off, road ahead; round the car ahead | 0.0259; 0.1546 | 0.0264; 0.1585 | 0.0261; 0.1557 | 0.0270; 0.1596 | |

The road beside the car is as bright as the JS draws it again, 3 to 7
times what the spot alone gave; the pools' shape, size and place match,
and so does the light they add (Sierra's phone boxes were already
brighter before the effects; what the pools add is +0.0155, +0.0138 in the
JS and +0.0144, +0.0155 here). The race frames are taken a fraction of a
frame apart, so a pool's edge moves a little between them; the whole-pool
boxes differ by 6 to 9 %. The native client draws the same pools
(Coast's countdown and 20 s). Pictures: `parity/report/effects/` (not in
git).

Frame cost: Coast at 15 to 20 s, 1280 × 800, uncapped, WebGPU, six
alternating runs each on a machine shared with other agents: median frame
3.0 to 6.4 ms with the effects and 2.9 to 4.4 ms without; the quiet runs,
3.0 to 3.2 ms against 2.9 to 3.3 ms, cannot tell them apart. Per frame the
effects cost 28 pool quads and, while particles live, one write of the
two rings (4,800 vertices, 211 kB) into the slab, as the JS uploads its
four attribute arrays each frame.

## D806. The JS never draws its skid marks; the port does the same

2026-10-04. `SkidMarks.add` lays each quad's corners as a + n, a − n,
b + n, b − n with n the travel turned left, and indexes it (0, 1, 2),
(1, 3, 2): both triangles wind clockwise seen from above, so their front
faces point down, and the marks' `ShaderMaterial` has three's default
side, FrontSide. Every mark is culled: none shows in the game, by day or
night (the staged `skid-day` scene, a hard skid on grey asphalt, shows
none). Drawn double-sided (a probe in the JS page, not in the game) they
appear as two dark trails (`parity/report/effects/scenes/js-skiddouble/`).
Port, don't improve: the Rust lays the same quads with the same winding
and culls the back faces, so it draws none either; DEVIATIONS lists it
with the JS's other quirks. Making them show is a one-line change on both
sides (the side, or the index order) and the owner's call; the marks are
otherwise ported and checked (`SkidMarks` tests, the shader).

## D807. The skid marks show, in both games

2026-10-04, the owner ("yes make skid marks visible"), on D806. A bug
fix the owner reported, so the frozen JS game takes it too (CLAUDE.md):
`SkidMarks`' index order becomes (b, b+2, b+1, b+1, b+2, b+3) in
`src/game/Effects.js` and `play::fx::skid_mesh`, so each quad's
triangles wind counter-clockwise seen from above, face up, and survive
three's FrontSide culling. Nothing else changes: same quads, alpha,
colour, blending and polygon offset. The two games still match each
other; the JS-tree key changes with the edit, so the parity cache
regenerates on demand. D806's entry in DEVIATIONS.md is removed.

## D808. The effects sort among transparent objects as three sorts them

2026-10-04, after D807 made the skid marks visible. In the staged
`skid-day` scene the Rust drew the marks over the smoke, darker; the JS
drew the smoke over them, so they read lighter grey. Neither the marks'
colour nor alpha differed: the order did. Two causes, the first the one
that showed:

- **Bevy sorts by the polygon offset.** `Material::depth_bias` is added to
  a transparent item's sort distance, and Bevy uses it for nothing else
  (the pipeline's bias is ours, set in `specialize`). `ThreeMaterial`
  returned its constant bias there, so every polygon-offset material
  sorted as if hundreds of metres nearer the camera: the skids by 256, the
  pools by 384. three's sort never looks at `polygonOffset`. The effects'
  materials now return a tie-break instead (`ThreeKey::sort_rank`, below).
- **Where three sorts the rings.** three sorts a transparent object by its
  geometry's bounding-sphere centre (the box's middle, every position
  counted, unused ring slots at the origin) through `matrixWorld`, then by
  object id; the sphere is computed the first time the object is rendered
  and never again. In a race that render comes before the first
  `effects.update` (a JS race at 3 s: smoke, sparks, skids and pools all
  centred on the origin, ids 3165, 3166, 3167, 3172), so the three rings
  tie and draw in creation order, the marks over the smoke. In the staged
  scenes the frames run before the first render, so each ring's centre is
  that of its contents, and the smoke sorts behind or in front of the
  marks by where it is. Bevy sorts by the render mesh's box centre (from
  the vertices it was made with, all zeros) through the entity's
  transform. Now `Fx::fix_sort_centres` takes three's centres from the
  buffers at the same moment (at the race's start; after the staged
  frames), each ring's entity sits at its centre and its vertices are
  written relative to it, so Bevy sorts it by the same point. Ties break
  by `sort_rank` (smoke 1, sparks 2, skids 3, flames 4, pools 5: the order
  `Effects` makes them, as ids), one `SORT_STEP` of 1 cm each, which f32
  resolves at a race's distances and nothing untied is that close.

`skid-day` against the JS, mean ΔE00 and 95 % block: 0.285, 0.587 before
(within the limits: the metric hides a local wrong order), 0.197, 0.390
now; the marks the same grey under the same smoke. The five staged scenes
on WebGPU and WebGL2, after D807 on both sides: pools 0.133, 0.331;
skid-smoke 0.965, 5.675; skid-day 0.197, 0.390; sparks-flames 0.139,
0.382; launch 0.235, 0.415 (WebGL2 0.422); all within. The night races
(D805's boxes, the JS frames as before) are unchanged within a frame's
motion: Coast desktop 0.0186, 0.0139 (WebGL2 0.0184, 0.0142) against
0.0184, 0.0134; Sierra desktop 0.0397, 0.0233 against 0.0372, 0.0212;
Sierra phone 0.0328, 0.0213; Coast phone 0.0266, 0.1598.

## D809. The scene materials still sort by their polygon offset

2026-10-04. D808's first cause holds for every scene material with
`polygonOffset` that draws in the transparent pass (13 of the JS's world
files set a polygon offset; Coast's and Desert's lamp pools are such
materials): `sort_rank` 0 keeps their old sort, the
constant bias (32 × −(factor + units), 128 to 384) added to their
distance. Where two transparent objects overlap, that can put the offset
one in front where three draws it behind. It is left as it is here,
because every level's stations passed with it and changing it needs those
stations run again; giving the scene materials a rank of their own (three
breaks ties by object id, which is the scene's node order) is the fix,
for whoever next runs the stations.

Fixed in D810.

## D810. The transparent pass in three's order, for every material

2026-10-04, fixing what D809 left open (a divergence is a port bug, SPEC
4.2's rule). three sorts its transparent list by `renderOrder`, then by
`z` (the sort point through `projScreenMatrix`, far first), then by
object id. The port now does the same for every material:

- **No polygon offset in the sort.** `ThreeMaterial::depth_bias`, which
  Bevy adds to the sort distance and uses for nothing else, is the sort
  rank alone (0 for scene materials, D808's ranks for the effects); the
  offset stays in the pipeline (`specialize`).
- **`renderOrder`.** The scene carries each node's `render_order` (the
  JS's 24 settings: sea 1, surf rings and foam 2, Beach's transparent
  ribbons 3, the desert's pools 2 and beams 3, the car glass 1 and
  siren glow 2, the city's pools, glows and haze, the waterfall's
  layers...), and the client ignored it. Drawn entities whose order is
  not 0 carry `render::sort::RenderOrder` (loader, `play::models`), and
  a render-world system after Bevy's `Transparent3d` sort sorts each
  view's items again, stably, by order, then by distance. A phase item
  names its main-world entity (its render entity is a placeholder), so
  the orders are looked up by that.
- **Behind the camera.** three's divide by w, negative behind the
  camera, puts a sort point there past the far plane (NDC z = (f + n)/(f
  − n) + 2fn/((f − n) z) > 1), so such an object draws before everything
  in front, nearer behind first. Bevy's view depth draws it last. The
  same system gives those items keys below every in-front one, in three's
  order (`sort::three_key`). In front, three's NDC depth orders as the
  view depth does, so Bevy's distance stands. Ties keep Bevy's order, the
  order the items were queued in, which is the scene's, as three's ids
  are.

The constant bias had been doing renderOrder's job by accident: it put
the surf and foam (polygon offset, order 2 or 3) after the sea (order 1).
Taking the bias out alone (the first try) sank the surf rings round
Coast's sea stacks under the sea at 01750 to 02750-high (mean ΔE00 0.156
to 0.365); with the orders and the behind-camera rule they are back.

All the L4 stations from the default (built) path, WebGPU, 1280 × 800,
frozen, against the JS shots (`rust-web-stations.mjs`; the JS tree key
changed with D807, which no fly-camera station draws, so the shots of
the key before it serve): 398 stations, all within the limits before and
after, median mean ΔE00 0.245 both. Two runs of the old build are
identical to the pixel, so every change is the sort's. 191 stations
change at all, 180 of them by under 0.005 mean (the largest change
against the JS among those 0.0045). The 11 that move more:

| Station | before | after (mean ΔE00 against the JS; 95 % block) |
|---|---|---|
| coast/00750-high | 0.393; 0.368 | 0.145; 0.297 |
| coast/00500-high | 0.258; 0.335 | 0.145; 0.286 |
| coast/00250-high | 0.161; 0.311 | 0.126; 0.279 |
| desert/07250-high | 0.183; 0.400 | 0.171; 0.373 |
| sierra/02000-chase | 0.216; 0.512 | 0.200; 0.480 |
| sierra/02000-high | 0.276; 0.708 | 0.270; 0.685 |
| coast/attract | 0.134; 0.327 | 0.129; 0.309 |
| desert/07000-high | 0.161; 0.364 | 0.161; 0.358 |
| desert/07000-chase | 0.134; 0.253 | 0.134; 0.253 |
| coast/00750-chase | 0.124; 0.312 | 0.139; 0.384 |
| desert/06250-chase | 0.127; 0.218 | 0.156; 0.340 |

Eight closer, three further. The big gains are Coast's lighthouse beam
(additive), which the sea (order 1) now covers where it passes over the
water, as in the JS; before, the beam drew over the sea out to the left
edge. The two that went further are small and local: Coast's 00750-chase
at the frame's left edge, and the campfire halos and ground pools of
Desert's 06250-chase (brighter than the JS by about 7/255 in 3,168
pixels); pools, points, sprites and beams there are all additive, so
the cause is the order against something normal-blended there, not
found yet. A WebGL2 spot check (Coast and Desert, 130 stations) gives
the same numbers as WebGPU (within 0.008 mean). The effects' staged scenes
(WebGPU) stay within and come a little closer: launch 0.235 to 0.127
mean, skid-smoke 0.965 to 0.954, the others within 0.004; Coast's night
race reads as before (0.0185, 0.0140 against the JS's 0.0184, 0.0134; in
a race the smoke, sparks and skids sort by the origin, which, when it
is behind the camera, now puts them first, as three does). Opaque
objects still
ignore `renderOrder` (three sorts its opaque list by it too, which
matters only where depths tie).

## WP 6.3 decisions: the race HUD

## D820. The HUD: an engine-free `HUD` class, and nodes built once

2026-10-04, roadmap WP 6.3 (`HUD.js`, the HUD half of `hud.css`). The
M4 text HUD (D431, D687) is replaced. `play/hud/model.rs` is the `HUD`
class without the DOM: `update(dt, st)` with the class's `set` (a text
is written only when it changed), `toast`, `center`, `radio`,
`setPursuit` and `updatePursuit`, the zone card on a zone change (once
started; on a circuit only on lap one), the route dots, the cruise
panel, the speed lines' opacity and which dial shows. Its `st` is
`Race.js`'s payload (`play/hud.rs` `hud_in`: `lapS`, the standings, the
active traffic, the rivals in standings order). `hud.test.js`'s
thirteen cases are its tests, with the same inputs and outputs, plus
the readouts, laps, cruise, zone card, toast timing and speed lines.
`play/hud.rs` lays the elements out and builds the nodes once per race,
screen size and kind (cruise, circuit, electric, touch); each frame it
puts the model's view on them, writing a text, width, colour or
transform only when it differs, so nothing is rebuilt and the layout is
redone only for what moved (the nitro fill, the route dots, the
multiplier bar). The model is a new `HUD` at each race start, with
`mph` from the settings and `bestScore` from the store (D687). Not
updated while paused (the JS loop skips `race.update`), shown in pause
and results behind the screens as in the JS, hidden while the race is
held (D574). The Hot Pursuit furniture (stars, bust and evade bar,
damage, penalty, hold card, radio) has its state in the model already
(tested) and `M8:` marks where its nodes go, in `index.html`'s order;
`hud_in` gives `pursuit: None` until M8's `hudState`. `?hud=0` turns the
HUD off (pictures and frame times without it).

`play/flow.rs`'s `Hud` gains two counters, `centers` and `toasts` (a
hook, told to its owner): each call restarts the pop or the toast. The
centre's class follows its text, one class per text as the JS's calls
give them (`GO!`, `n PLACE`, `WINNER!`, `ESCAPED`, `TAKEDOWN`: `pop
go`; `WRONG WAY`, `PURSUIT`, `SPIKED!`, `BUSTED`, `WRECKED`: `warn
pop`; the rest `pop`). The M4 controls line under the countdown and the
hidden placeholder panel are gone: the JS HUD has neither, and the menu
shows the controls.

## D821. The canvases and the speed lines: one UI material

2026-10-04. SPEC 8.1 asks for the dials and minimap as generated 2D
meshes and the speed lines as a UI shader. Bevy UI draws no meshes, and
a second camera rendering meshes to a texture would add a pass and its
own render graph beside the post chain; so all three are one
`UiMaterial` (`play/hud/material.rs`, `hud.wgsl`), told apart by a
uniform, so one pipeline and one copy of the material plugin. The
canvases' paths become signed distances, antialiased over a pixel:
the dial's backplate, track, redline (or regen) zone, the fill with the
canvas's diagonal gradient (`createLinearGradient(0, W, W, 0)`, stops
in sRGB) and the ticks; the minimap's road (s − 500 to s + 900 every
6 m and the runout, two strokes with round caps and joins), the finish,
traffic, rivals, the police bars and blinking units (ready for M8), the
clip circle and the player's arrow; the speed lines' repeating conic
gradient and radial mask. The CPU side computes the canvas's own
transform (`play/hud/minimap.rs`, `dials.rs`, tested: the projection,
the road samples, culling to the circle, the dial angles, labels,
needle and gradient) and rewrites the uniform only when it changed. The
dial's labels are text nodes and its needle a turned node above the
material, as the canvas draws them last. `N₂O` is N, a small low 2 and
O (Rajdhani has no ₂); its `mix-blend-mode: difference` is a colour
switch when the fill reaches under it. The nitro pulse is the
gradient's brightness from the keyframes.

## D822. Text shadows, glows and the minimap's shadow

2026-10-04. Bevy's text shadow is a hard copy with no blur. A CSS
`text-shadow: 0 y b rgba(0,0,0,a)` becomes a copy y px down with alpha
`a · 2/b`: at full alpha the copy read as a second glyph on Seaside's
light sky, where the blurred one is a soft halo. The toast's
`0 0 14px` blue glow has no hard equivalent and is left out. The
minimap's `box-shadow` ring is an outline; its 24 px drop shadow is left
out (Bevy draws a box shadow under the whole node, and the disc is
translucent, D573).

## D823. Translucent colours on a linear target

2026-10-04. The page composites in sRGB; Bevy blends the UI into the
post chain's linear Rgba16Float target, where a translucent colour shows
more of what is behind it: the minimap's dark disc came out at (95, 88,
78) against the JS's (61, 58, 59) over the same rock. The HUD's
translucent colours go through `hc()` (and the material's output
through the same `lin_alpha` in `hud.wgsl`): the alpha that, blended in
linear, lands where the sRGB blend would over a background of sRGB
0.25, and close to it over others for the dark panels (black at 50 %
over sRGB 0.6: 0.28 of the background's light against 0.23); after it the disc reads (76, 70, 64) and the phone's over
the sky (57, 71, 98) against (49, 61, 85). The menus' screens (WP 6.2)
have the same effect and do not correct it; `hc` is in `play/hud.rs`
for them to take if wanted.

## D824. The CSS animations from the real clock

2026-10-04. The browser runs `pop` (.8 s), `zoneCard` (3.6 s), the
toast's .25 s opacity transition and `nitroPulse` (.18 s alternate) on
wall time, also while paused; the port computes them from Bevy's real
time with the CSS timing functions (`ease`, `ease-out` per keyframe
interval; `fill-mode: both`), tested. Opacity on an element is its
texts' and shadows' alpha; scale and slide are `UiTransform`s, so they
move no layout.

## D825. The layout at the CSS's breakpoints

2026-10-04. Desktop; `max-width: 720px` (position 44 px, minimap 120,
the dial box scaled .7 from its corner, the route bar bottom left at
50vw); the touch layout (`body.touch`: the small minimap at the safe
insets, the route bar 30vw at the top in landscape and 70vw at 166 px in
portrait, the compact speedo box at `--steer-top` + 8, the cruise panel
at .7, centre text 84 px for every class, which outranks `.warn`'s 54
in the CSS). The `.pos` line is a row placed on Rajdhani's metrics
(ascent .93, line 1.276 em: the `sup` at the line's top plus 8 px, the
count on the baseline). `--steer-top` follows the stick, buttons or
tilt steering that steers right now (`race.touch.steering`, from the
touch package, D840+; merged at the two packages' merge).

## D826. L4: the HUD beside the JS

2026-10-04. `tools/parity/hud-shots.mjs` takes the JS game (Rajdhani
for its Google Fonts request, `?parity=1`, stopped at the tick) and the
Rust web build (the registered server, the page shooting the first frame
at that race time) at the same race time, seed 1, the autopilot;
`--countdown` for the pop; `hud-crops.py` puts parts side by side.
Pictures in `parity/report/hud/` (not in git), `index.html` there.
Sierra 20 s on desktop 1280 × 800, iPhone portrait 390 × 844 and
landscape 844 × 390; Sierra 101.5 s (the Old Mill Valley card and a
NEAR MISS toast); the electric car's power meter; Seaside 40 s (the lap
line) on desktop and phone landscape; the Cruise at 30 s (score panel,
speed lines) on desktop and phone portrait; the countdown's 2; WebGL2 at
Sierra 20 s desktop and phone and 101.5 s. Every element lands within a
pixel or two of the JS's, in the same font, size, weight, spacing and
colour; the dial, its labels and needle, the minimap and route bar match
mark for mark. What differs: the shadows and the toast's glow (D822),
`N₂O` in the toasts is N2O (the flow's text, D431), the touch buttons'
icons (the touch package's), and the scene behind (the race positions
differ by a few metres between the builds at the same time). WebGL2
draws the same.

Frame cost, Sierra at 40 s, 1280 × 800, WebGPU, uncapped (median rAF
interval over 6 s, three alternating pairs on a machine at load 60+):
with the HUD 8.0, 7.5, 4.5 ms; with `?hud=0` 8.0, 6.8, 4.2 ms, so about
0.3 ms in the quiet pair, within the runs' spread. Per frame the HUD
writes one 6 kB uniform each for the dial and the minimap when they
change and a few texts, and builds no nodes.

## WP 6.5–6.6 decisions: touch controls, tilt and the gesture bridge

## D840. The touch controls whole

2026-10-04, WP 6.5. `play::touch` now ports all of `TouchControls.js`
(D436 had the stick and the slider): the steering `mode` (`Steering`:
stick, buttons, tilt) and what steers right now (`steering`, `layout()`:
tilt only once the sensor is live, the stick standing in), the pedals
(`PedalKind`: slider, buttons), the ◂ ▸ pads and the GAS, BRAKE, DRIFT
and N₂O pads with their 14 px slop and nearest-centre pick, the
`pointers` map (insertion order kept, a finger moved keeps its place)
re-hit-tested on every move so a thumb slides from pad to pad, a finger
on a tap button also going into `pointers` as in the JS, `setPedals`
letting go of every finger, auto gas (`autoGas && visible && !brake &&
!slide`), the wheel's `s × 90°`, and the haptic tick (`navigator.vibrate(8)`,
asked for with `buzz` and made by the web glue the same frame). A pad that
is hidden (the ◂ ▸ pads unless steering is on them, the pedal pads with
the slider) has no box, as its `display: none` gave a zero rect.
`touch::Layout` adds the boxes `hud.css` gives `.t-steer`'s two 1.08 b
pads (14 px apart at `--inL`, `--inB`), `.t-pedals`' grid (two b-wide
columns 14 px apart, rows 12 px apart, items at the bottom of their row and
centred: DRIFT and N₂O 0.74 b round over BRAKE and GAS b × 1.3 b) and
`.t-wheel` (1.3 b at `--inL + 8`, `--inB + 8`). The settings reach the
race's controls through `play::tilt::sync`, which reads `UiState.settings`
each frame (as `gamepad_io` reads the rumble switch): a new race's
controls take steering, pedals and auto gas, and after that a setting is
applied when it changes (the JS's `onchange`s), so a test can set
`autoGas` on the controls directly as on `__race`. The race's controls are
set one frame after the race is built (the countdown's first frame steers
with the defaults, which nobody can touch then).

## D841. `TiltSteer` over a window trait; the promise, the timer and the events

2026-10-04, WP 6.6. `play::tilt` ports `TiltSteer.js` (`screenRoll`,
`rollToSteer`, `fullLockFor`, the seven states, `enable`, `listen`,
`onOrientation`, `update`) through the kernel's trig, `pow` and `exp`.
The JS's `win` is the `TiltWindow` trait: `DeviceOrientationEvent` there
or not, `requestPermission` a function or not, ask (false if it threw),
add or remove the one listener, `isSecureContext === false`. The web side
(`tilt::web`) reads them with `js_sys::Reflect`; natively there is no
sensor (`NoSensor`: tilt chosen reads 'none' and the stick steers, SPEC
8.4). What was asynchronous in the JS comes in at the next frame, in
order: the promise's outcome as `answer(Granted | Denied | Failed)` (the
JS's `then` handlers, same branches), the `deviceorientation` events as
`on_orientation(beta, gamma, angle)` with the angle read when the event
fired, and the 2 s `setTimeout` as a deadline `poll(now)` checks on
real time. There is one `TiltSteer` for the run, shared with each race's
controls as `Arc<Mutex<…>>` (the JS's `Object.assign(touch, { tilt })`),
so its state and the menu's note carry from race to race. Its smoothing
runs per tick from the input layer (D433) where the JS ran it per frame;
the exponential step composes exactly, so only the 1e-3 snap can land a
tick apart. Every `tilt.test.js` case is a Rust test, with the JS's
`pose()` helper ported, plus the 2 s wait.

## D842. Drawing the controls: Bevy UI, the SVG icons with `mp_canvas`

2026-10-04, WP 6.5. `play::touch_ui` draws every part of `#touch`: the
stick (its track at .75 opacity while idle, the knob's border white when
held and the accent at full lock), the ◂ ▸ pads, the wheel (an image
turned by `UiTransform`'s rotation, clockwise as CSS `rotate`), the
slider with its bands, mark, fills and knob (with its glow), the DRIFT
strip, the four pedal pads with their colours, and the three tap
buttons, `.on` as the CSS has it (white border, the lit background,
`scale(.94)`). The icons are the page's SVG paths (◂ ▸, reset, camera,
pause, the wheel) stroked with `mp_canvas` into 96 px images once, at
the SVG's stroke width and round caps; no font has them (D431's R, C,
II and `<` `>` are gone). Font sizes, icon sizes and border widths are
CSS px divided by the page's scale (`css_scale`), as positions already
were: at a pixel ratio of 1 on a 2× or 3× phone (High quality off, the
phones' default) the labels had been drawn at half or a third of their
size. Left out: the `drop-shadow` filter and the text shadows, and the
letter spacing (Bevy text has none).

## D843. Motion permission inside the tap

2026-10-04, WP 6.6. iOS Safari grants `DeviceOrientationEvent.
requestPermission()` only inside a user gesture, and a Bevy frame is not
one. The JS asks in `opt-steer`'s change handler and in `startRace`. The
page's gesture bridge (`ui::web::gesture_at`, D577) now also asks when a
tap lands on Race, Race again or Restart while tilt is the steering
choice on a touch screen, or on the steering drop-down's Tilt option
(`option-tilt`); the request is the same `tilt::web::ask` that
`TiltSteer::enable` makes from the frame, whose answer reaches the
`TiltSteer` at the next frame. Asked from a frame (the menu's change, a
race's `startRace` call, the start-up `enable(true)`), an iPhone rejects,
which is the 'ask' state and the note "Tap Race to allow motion access",
as in the JS when a page loads with tilt saved. One request is in flight
at a time: while it waits, another `enable` does not ask again (the JS
would have, from the menu's change and then the frame's `startRace`, a
second prompt behind the first); and once granted, the gesture side does
not ask again in that visit, as `granted` stops the JS asking. The motion request is made before the fullscreen request in the same
handler: a fullscreen request uses up the tap's activation in Chrome, so
a browser offering both would refuse the motion request after it (an
emulated iPhone in Chrome did); the JS asked after `enterFullscreen`,
which on an iPhone, with no element fullscreen, comes to the same.
`startRace`'s own `tilt.enable(true)` is made once a race is shown (no
screen up, not held), not for the menu's warm-up field behind the
loading screen (D574), which would have asked a second time at load.
Android Chrome has no `requestPermission` and listens at once. Checked
with a faked `requestPermission` that says yes only while
`navigator.userActivation.isActive`: the note at load, then granted in
the Race tap, and granted in the tap on Tilt (`tilt.test.mjs`).

## D844. Fullscreen and the landscape lock

2026-10-04, WP 6.6. Unchanged from D577: the Race, Race again and
Restart taps on a touch screen with Fullscreen on call
`requestFullscreen({ navigationUI: 'hide' })` (or the webkit one) and
then `screen.orientation.lock('landscape')`, all best-effort, as
`enterFullscreen` does. iPhone Safari has no element fullscreen, so there
`requestFullscreen` is missing and nothing happens, and it has no
orientation lock either; the JS does the same, and shows the menu's
"turn your phone sideways" hint in portrait (already in `ui::menu`). The
page's viewport now has the JS's `maximum-scale=1, user-scalable=no`, so
a double tap on the controls cannot zoom an iPhone's page.

## D845. The visibility pause, latched

2026-10-04, WP 6.6. `play::web` paused the race when it found
`document.hidden` in its `Last` system. A hidden page gets no animation
frames, so that check could miss the hide altogether, and in the first
frame back it ran after that frame's ticks. Now the page's
`visibilitychange` (to hidden) is latched by a listener, and a system
before the race's frame pauses the race when the latch is set or the
page is hidden (`if (document.hidden && mode === 'race') pause(true)`),
so the race is paused before any tick runs on the return. The pause
screen hides the controls, which lets go of every finger, as
`showScreen('pause')` does. Natively focus loss still only lets go of
the keys and fingers (D106).

## D846. The controls suites against the Rust build

2026-10-04, WP 6.5, 6.6. `touch-controls`, `analog-controls` and `tilt`
are adapted in `tools/parity/e2e/` with `controls-helpers.mjs`:
`__race` is `__mp.race`, which gains the car's `yaw`, `steerAngle`,
`hw`, `yawToRoad`, `nitro`, `nitroActive`, `camMode`, the camera's right
(`camRight`, `__camera.matrixWorld`'s x axis) and `touch` (the controls:
`visible`, `mode`, `steering`, `pedals`, `autoGas`, `stickR`, `held`,
`stick`, `lock`, `knob`, `panel` (the pedal panel's classes), `wheel`,
`tilt`); `__mp.race.touchUi` is the old boolean. The pads are
`__mp.ui('touch-<act>')` (`touch-left`, `touch-throttle`, …, and
`touch-stick`, `touch-slider`, `touch-drift`, `touch-pedal`,
`touch-wheel`), not visible when hidden. `__mp.stage` gains `place`
(`phys.reset` and the speed, with `fromFinish`, `latFrac`, `yaw`, the
camera snapped), `nitro` and `autogas`. A drop-down choice is two taps
(the select, then `option-<value>`), the sensitivity slider a drag to an
end, and the tilt suite gains a real hidden page (another tab brought to
the front) for the visibility pause and the iPhone permission case
(D843). `tools/parity/touch-shots.mjs` puts the JS and the Rust controls
side by side on an emulated iPhone, sideways and upright (at rest, both
thumbs down, the Buttons choices held, tilt turned), in
`parity/report/touch/`. All three suites pass against the release build
(touch-controls 10, analog-controls 5, tilt 7), as do `race-flow` and
`race-button`. Chrome logs "Ignored attempt to
cancel a touchstart event with cancelable=false" now and then when a CDP
touch arrives while a frame is busy (winit cancels every touchstart on
the canvas); the suites leave that line out of their error check, since
the canvas is `touch-action: none` and nothing scrolls or zooms either
way.

## D751. Race from the menu builds the level first (the owner's D750 choice)

2026-10-04, the owner's option 2 for D750: build the level before the
race's cars and sound come up, so a race started from the menu peaks no
higher than the same level raced from the address. The boot and the Race
tap change, in `ui` only:
- **No field behind the menu.** Over the menu's views no race field is
  built at boot (`play.armed` false when `preview::wanted`), so D574's
  warm-up field and the sound's graph built with it (D580) are gone from
  the menu: the cars come up only for a race, after its level, behind the
  loading screen, where Race's hold (D574: three quiet frames, at most
  three seconds) compiles their pipelines as it does after a level switch.
  The sound's graph is built by the first gesture, as `wakeAudio` builds
  it in the JS (the music on the menu starts with the first tap, as
  there).
- **The views go first.** Race frees the views and waits four frames
  (`Starting::Free`, `FREE_FRAMES`) for the despawned assets to be
  released before asking for the level; the level is then built, then the
  field and the sound's car, as from the address.
Measured on WebGPU at load 38 to 66 with `compare.mjs` (Coast from the
menu and from the address, alternating, three rounds each): before this,
515 to 564 MB from the menu against 466 to 507 from the address (D749);
with D751 to D753, 490 to 522 (median 490) against 462 to 510 (median
462). WebGL2 at load 86 to 121: 518 to 535 (median 521) against 453 to 501
(median 496). The menu's own high-water drops from 167 to 173 MB to 139
to 151 (no field). `sections.test.mjs`, `level-switch.test.mjs` and
`race-flow.test.mjs` pass on WebGPU and WebGL2 (the last two take
`MP_BACKEND=webgl2` now); the native menu-to-race script too.

## D752. The menu's glyph atlases go with the views

2026-10-04. Bevy keeps a CPU copy of every glyph atlas it has drawn text
into. The menu, drawn at up to twice the CSS resolution (D575) in many
sizes, held about 20 MB of them (`FontAtlasSet::total_bytes`, a diagnostic
build). When Race frees the views it clears the atlas set and marks every
`Text` and `TextSpan` changed, so the texts that stay are laid out again
from fresh atlases on the next frame (`preview::release_glyphs`).

## D753. The page's loading screen covers Race's load; none under it

2026-10-04. On the web the page's own loading screen covers the canvas
while the client loads (D576), and the client drew its Bevy copy under it,
whose glyphs (about 8 MB at the screens' ratio) were then held through
the level's build. On the web the client now draws no loading screen
(`ui::build`, `Screen::Loading`; `__mp.screen` still reads `loading`), and
Race puts the page's up at once (`__mp.cover`, from `preview::cover`)
while the views are freed, before `__mp.reload` keeps it up for the load.
Natively the client's loading screen is drawn as before.

What is still above the address's figure (about 25 to 30 MB at the
median): counted with a heap-counting allocator (a diagnostic build, not
committed), the heap in use when Coast's build starts from the menu is
about 77 MB against about 43 MB from the address. The sound's graph (about
12 MB) is built by the first tap, on the menu or on Race, and cannot wait
for the level: on a phone the context must be made and resumed inside a
gesture. The rest is what drawing the menu leaves in the renderer (the
compiled pipelines and the shader cache for the views' materials, the
renderer's grown buffers), which Bevy does not release. Not closed further
here; for the owner if the remaining 25 to 30 MB matter.

## Frame-time decisions (SPEC 6.6, "no worse than the JS game")

## D860. How frame time is compared with the JS game

2026-10-04. SPEC 6.6 asks for frame time no worse than the JS game on the
same device and settings. Until now both games were compared by the
interval between `requestAnimationFrame` callbacks, uncapped (BASELINE.md
from WP 2.6 on). On this machine that interval measures whichever stage is
slowest at the moment, and the stages are shared: the CPUs with other
agents (load average 8 to 130 during this work) and the GPU (other users
kept it at 100 % utilisation before any of these runs started). So
`tools/parity/rust-perf.mjs` now times each frame's work from inside the
page, the same way for both games, and the comparison uses CPU time:

- **rAF callback time** ("raf" below): every `requestAnimationFrame`
  callback is wrapped before the page's scripts run, and the callbacks of
  one frame are summed. Both games do all of a frame's work there: the JS
  game's `frame` (simulation, `world.update`, three's render), and Bevy's
  whole `App::update` (winit runs it from its animation frame; there is no
  other per-frame task). This is wall time, so it includes the thread
  being descheduled under load and waiting on the GPU process.
- **Main-thread CPU per frame** ("main"): `--trace N` records N seconds of
  Chrome's trace (`toplevel`, `devtools.timeline`, aggregated as the events
  arrive) and divides the renderer main thread's thread time (`tdur` of
  its top-level tasks) by the frames. Thread time leaves out time the
  thread was descheduled, so this is the figure least moved by the load.
- **GPU-process CPU per frame** ("gpu"): the same for the GPU process's
  main thread (`CrGpuMain`), where Chrome decodes and runs the page's WebGL
  or WebGPU commands, with its busy share; and "all", every traced thread.
- The **load average** is sampled every two seconds through each run;
  builds alternate (JS, the build before, the build after) level by level.

Settings as BASELINE.md's: 1280 × 800, high quality, the fly camera at
60 m/s from s = 80 (JS `?s=80&v=60&h=5&back=14`; Rust the measurement page
`?perf=1`), uncapped, plus runs at 60 Hz (`--capped`), a phone (844 × 390
at dpr 3, touch, `--hq 0`, CPU throttled 4 ×: `--phone --throttle 4`),
WebGL2 (`--backend webgl2`) and races with the autopilot (`--race`: the
sports car, seed 1, no pursuit, timed from race time 3 s, audio allowed
without a gesture in both games). The Rust flights pass `cars=0`: the JS
fly camera has no cars, while the Rust one adds WP 2.4's thirty stand-in
cars by default (`cars_on`), which the comparison should not carry.

Profiles: Chrome's sampling profiler over a copy of the release wasm with
its function names kept (`wasm-opt -Oz -g` on the same code), mostly at
60 Hz so the page is not blocked on the GPU process; WebGPU (and WebGL)
calls per frame counted by wrapping the `GPU*` prototypes; the GPU
process traced with Chrome's `gpu` and `dawn` categories. The native
client could not be profiled with `perf` here (`perf_event_paranoid` is 4
and there is no root), and Bevy's `trace_chrome` feature needs crates
that are not in `Cargo.lock`; the wasm profile in the browser is the one
that matters for the web, and is what the findings below come from.

## D861. No light clustering

2026-10-04. Bevy clusters point lights, spot lights and decals for its
own PBR shaders. Where the device has compute (WebGPU) it does it on the
GPU every frame: a z-slicing compute pass, a raster pass and an
allocation pass per view, and a staging buffer mapped (`mapAsync`) to
read the counts back for the next frame. The client has no Bevy point or
spot lights: three's lights (the sun, the hemisphere, the headlight spot,
the police point light) reach `three_std` through the globals texture
(D293, D456), and no material reads Bevy's clusters. In a profile of
Sierra's flight the clustering systems and the readback were about a
third of a WebGPU frame's main-thread time (1.24 ms of passes and 1.35
ms in `mapAsync`, of 7.9 ms, uncapped at load 30), and two of the
frame's render passes and four of its compute dispatches. Now a Startup
system sets `GlobalClusterSettings::gpu_clustering` to `None` (Bevy's own
switch, for devices without compute) and the camera carries
`ClusterConfig::None`, so the CPU path that replaces it has no clusters
to fill (WebGL2 always took that path, and now does nothing there
either). Sierra, A/B alternating at load 40: main-thread CPU 1.71 to
1.42 ms a frame, GPU process 2.62 to 2.10 ms. Pictures: unchanged, as
for D862 to D865 (BASELINE.md, "Frame time against the JS game":
stations, WebGL2, effect scenes and HUD shots against the build
before).

## D862. Bevy's `render_system` without its empty submission

2026-10-04. After the render graph has submitted the frame,
`bevy_render::renderer::render_system` makes a second command encoder for
screenshot copies and GPU readbacks and submits it every frame, empty or
not. In Chrome every submission is a `Queue::Submit` and a `vkQueueSubmit`
in the GPU process, which sets an uncapped WebGPU frame's pace (its main
thread was busy 100 % of the time in every Rust run, against 50 to 90 %
for the JS game). `render::frame` takes `render_system`'s place in the
`Render` schedule (removed with `remove_systems_in_set`, the new system
ordered after `RenderSystems::Render`, which then holds only the
pipeline cache's queue processing, and before `Cleanup`): while an entity
with `Screenshot` or `Readback` exists in the main world it calls Bevy's
`render_system` itself, so screenshots and readbacks work as before;
otherwise it runs the render graph and presents, as `render_system`
does, without the extra encoder. Sierra, three alternating pairs at load
20 to 50: GPU-process CPU 2.83, 2.49, 2.80 to 2.56, 1.80, 2.09 ms a frame,
all threads 3.84, 3.59, 3.84 to 3.52, 3.22, 3.30 ms. It copies Bevy
0.19.1's present loop; a Bevy upgrade (D100) must check it against its
`render_system`.

## D863. The level ids are kept, not rebuilt every frame

2026-10-04. `Options::race_on` asked `mp_levels::levels()` whether the
level is one of the menu's, and that builds all six levels (Downtown
Streets' route among them) each call; `fly_system` and `cars::start_cars`
call it every frame (0.17 ms a frame of wall time in the profile above).
`options::is_level` keeps the ids once (`OnceLock`); `make_track` uses it
too.

## D864. The gamepad bridge is written when it changes

2026-10-04. `gamepad_io::web::publish` built `window.__mp.pads` (about 40
properties in four objects) every frame for the tests (WP 6.4), about
0.09 ms of a race frame's wall time, more than reading the pads. It now
remembers what it wrote (the pads' state, the pad in hand, a capture,
rumble and the reset label) and writes only when that differs. The tests
read the same object with the same contents.

## D865. The HUD's uniforms are written into their buffers

2026-10-04. The dial's and the minimap's uniforms (D821, 6 KB each)
change most frames of a race, and the HUD wrote them by editing the
`HudMaterial` asset, which Bevy answers by preparing the material again:
the uniforms encoded, a new buffer created mapped, a new bind group, the
old ones dropped (D455's cost; in Firefox the per-frame GPU objects drive
the GC pauses of D457). About 0.12 ms of a race frame's main-thread wall
time, plus the GPU process's share. `HudMaterial` now implements
`AsBindGroup` itself (the layout is the derive's, from a one-field
struct): its bind group is made once over a uniform buffer per material
that the render world keeps (`UNIFORM | COPY_DST`), and `hud::update`
hands new uniforms to `HudWrites`; they are extracted and written into
the buffer (`write_buffer`) after the materials are prepared
(`PrepareResources`), so the same bytes (encase's encoding, as the derive
uses) reach the GPU in the same frame as the edit did.

## D866. Where the Rust client stands against the JS game, and what is left

2026-10-04, after D861 to D865 (numbers in BASELINE.md, "Frame time
against the JS game"). Per frame, the main thread's CPU time:

- **At 60 Hz on the desktop** (as browsers run it), the Rust client now
  spends less than the JS game on the two heaviest levels (Sierra 0.90
  against 1.12 ms, Coast 0.96 against 1.53) and 10 to 20 % more on Desert,
  Streets and the Cruise (0.93 against 0.78, 0.78 against 0.72, 0.79
  against 0.69); Seaside, the lightest, is 0.78 against 0.36. Before:
  1.09 to 1.40 ms on every level.
- **In a race at 60 Hz** (Sierra, Coast): 2.25 and 2.23 ms against the
  JS's 2.34 and 2.00 (before: 2.69, 2.88).
- **The GPU process** (Chrome's, which decodes and runs the page's GPU
  commands): 1.0 to 1.3 ms a frame against the JS's 0.4 to 1.1 when
  flying, 1.9 to 2.0 against 1.3 to 1.5 in a race (before: 1.4 to 1.7, and
  3.5 to 3.6 in a race).
- **A throttled phone** (4 ×, 844 × 390, high quality off): the page's
  frame work is 9 to 12 ms of the 16.7 against the JS's 3 to 9.5; Sierra
  and Seaside fell from 13.5 and 11.2 ms; Coast did not move (one run each).
- **Uncapped**, where the slowest stage sets the pace, the Rust client
  runs at 85 to 95 % of the JS game's frame rate on Sierra, Coast and
  Desert (105, 102, 100 against 120, 109, 115 fps) and below it on the
  light levels (Seaside 154 against 200, Streets 148 against 171), because
  its GPU-process thread is busy all the time.

What is left, and why it is not closed here:

1. **Bevy's fixed cost per frame.** About 360 systems run each frame
   (Bevy's main world, extraction, the render world's prepare and queue
   systems, the render graph) whatever the scene holds; on the light
   levels this is most of the frame (Seaside: 0.78 ms against the JS's
   0.36). About a tenth of it is systems of features the client never
   uses (morph and skin batching, decals, light probes, atmosphere,
   volumetric fog, screen-space reflections, order-independent
   transparency, mip generation, motion-vector history, point-light
   visibility): about 0.1 ms a frame on this desktop. They live inside
   `PbrPlugin` and `CorePipelinePlugin`, so leaving them out means
   removing systems by name after the app is built (each checked for
   what reads its output) or a patched Bevy; not done.
2. **WebGPU's cost per command in Chrome's GPU process.** The Rust client
   issues about 500 draws a frame on Sierra, as the JS game does (310
   indexed, 187 not; the JS 310 and 186), but about 1,000 WebGPU calls
   against about 3,200 WebGL calls, and Dawn validating and recording
   each draw and submission costs more than ANGLE's path: the GPU
   process's thread is the frame's bottleneck when uncapped. It shows in
   the page too: at 60 Hz the Rust callback takes 2.0 to 2.6 ms of wall
   time for 0.8 to 1.0 ms of CPU, the rest spent waiting on calls into the
   GPU process (the JS game's callback waits proportionally less). Less
   work there means fewer draws or commands (render bundles, merged
   draws), which is a renderer redesign, not a setting.
3. **`RenderDevice::limits()` per draw.** Bevy's `SetMeshBindGroup` asks
   for the device limits on every draw (`skins_use_uniform_buffers`), and
   wgpu's WebGPU backend answers by reading every limit from the
   browser's `GPUSupportedLimits` object (`map_wgt_limits`): about 0.08
   ms a frame of main-thread time on Sierra. A one-line cache in wgpu
   would remove it; that is a patched dependency, the owner's call (D674
   item 5's reasoning), and worth reporting upstream.
4. **WebGL2.** The fallback's main thread is 1.1 to 1.4 ms against the
   JS's 0.7 to 0.9, and uncapped it runs at 28 to 37 fps against the JS's
   100 to 110 on this desktop (the same before these changes): its GPU
   side (wgpu's GL backend through ANGLE) is the limit, as D456 described
   for Coast. Not investigated further here.
5. GPU preprocessing stays on: off (`?gpupre=0`), Sierra's main thread
   went from 1.4 to 5.9 ms a frame (19 fps).

## D867. Open, for the owner: `simd128` on the WebGPU build

2026-10-04, D674 item 2 measured again on the current client. The
WebGPU build with `-C target-feature=+simd128` (and wasm-opt's
`--enable-simd`): 0.16 MB smaller after gzip (8.83 against 8.99 MB), and
at 60 Hz, two alternating rounds at load 6 to 7, the main thread's CPU a
frame 0.88 and 0.87 ms against 0.99 and 0.88 on Sierra, 0.75 and 0.76
against 0.82 and 0.78 on Seaside: about 0.05 ms (5 %) less, within the
runs' spread. Every browser with WebGPU has wasm SIMD, so on the WebGPU
build it costs nothing; on the WebGL2 build it would stop the page
loading on Safari before 16.4. Proposed: the WebGPU build only, which
needs `xtask web` to pass the flag to that build's own target directory
(as D391 does for the WebGL2 cfg). Not adopted here.
## D679. A level viewer ("god mode"), Rust only

2026-10-04, the owner: "add a 'god mode' level viewer to the spec and
kick it off, i want to review levels from a different perspective". SPEC
8.6 and ROADMAP WP 6.9 describe it: free fly, orbit, overview and ride
cameras over the built level, a panel (route position, time of day,
fog, far plane, animators, scene groups, readout, screenshot, a link
carrying the pose), on keyboard and mouse, gamepad and touch, entered by
`?view=god` or a "Level viewer" button on the menu's level card. It is an
addition, not a change to anything the JS does: the game, the simulation
and the parity pictures are untouched, so it is not a deviation; the
menu button is listed in DEVIATIONS.md as the one visible difference on
a JS screen. Its decisions are D880 to D899.

## D880. The level viewer is a run of its own

2026-10-04, WP 6.9 (SPEC 8.6, D679). `?view=god` on a level (natively
`--query "view=god&level=coast"`) is never a race (`Options::race_on`
is false, so no menu, no race field, no sound graph) and has no stand-in
cars unless `cars=1` (`cars_on`): the level is built whole as for the fly
camera's runs (the client's own build, D678; `world=export` as
elsewhere), and `crate::viewer` drives the camera instead of
`fly_system` (which returns at once in a viewer run). Everything is in
`crates/mp_game/src/viewer/`: `cams` (the cameras as plain math), `link`
(the query string), `input`, `panel`, `groups`, `stats`, and `web` for
the page. Shared files gain only additions: `options.rs` (`viewer()`),
`lib.rs` (the module, its plugin, the early return), `animate.rs`
(`SceneIndex::groups`), `play/gamepad_io.rs` (`pads_only`), `ui` (the
menu's button and `Act::Viewer`) and the page (two lines). A race, the
menu, the simulation and every parity picture are as before: none of
this runs outside `view=god`, and the menu's button sits out of the
layout (D881).

## D881. The menu's button opens the viewer by address; Menu goes back the same way

2026-10-04. "Level viewer" is a pill in the top right corner of the
menu's level card, positioned out of the layout, so nothing else on the
menu moves (`btn-viewer`; DEVIATIONS.md). It opens
`?view=god&level=<chosen>` (keeping the page's own parameters such as
`backend`, `hq`, `world`), a page load, rather than switching the running
client from its menu views to a whole level and a free camera: the menu's
flow (D743, D751) stays untouched, and the viewer always starts from the
same state a link gives. The cost is the page load (the wasm is cached;
the level is built as Race builds it). The panel's Menu goes back to the
page without the viewer's parameters, with the viewed level saved as the
menu's choice (`mr.level`). Natively the client starts itself again with
`--query` and exits (no in-process switch either).

## D882. The cameras and their controls

2026-10-04. A pose is a position, a yaw about the world's up (0 looks
along −Z) and a pitch, rotation `Ry(yaw)·Rx(pitch)`, no roll; the link's
`cam=x,y,z,yaw,pitch` is that pose. Free fly moves where it looks (strafe
level, rise and sink straight), 1 to 800 m/s on a logarithmic slider
(25 by default), Shift ×4, Alt ×¼, its velocity eased (10/s) so a key tap
does not jerk. Orbit circles, tilts (−89° to +34°) and zooms (2 m to 30
km) about a centre that the keys or stick move, faster farther out. The
overview looks straight down (a perspective camera at the game's 62°)
from the height that shows the route's box whole, the map turned by yaw;
a drag moves the ground with the finger. The ride is `fly::fly_camera`
with its parameters adjusted live (speed, height, side, look, distance
behind). Changing camera keeps the view where it can (free ↔ orbit about
the ground point ahead, at most 3 km), and flies (an eased 1 to 1.6 s
flight) into and out of the overview and into the ride. Mouse drags turn
toward the motion (free, ride), as desktop viewers do; a finger grabs
the view (Street View's way); orbit drags move the scene with the
pointer. The bindings are in `viewer/input.rs`'s header and the panel's
"Controls".

## D883. The world's focus is the camera

2026-10-04. Each frame the viewer does what the game's frame does around
the player's car, around the camera: the focus (`CameraState::focus`,
the sky dome's centre, the sun's ±70 m shadow box) is the camera's
position (the ride keeps the fly camera's `frame(s − back)`, as
`main.js`), the animators get the camera (distance-shown nodes, the city's
fades, LOD, point sizes) and the nearest route position as their `s`
(`World.update(dt, s, focus)`), and `Sky.update` is called with that `s`,
so the time of day follows the camera along the route (on a loop it is
the middle, as in the game). The nearest route position is the closest of
every eighth sample, then the Track's own projection. Pinning the time of
day sets `Sky`'s `override_p` (the game's `?t=`, which the link reuses).
Animators frozen is the game's `?freeze=1` (world dt 0: the sky's clock,
the sea, the scenery all hold).

## D884. Picking a point on the ground without the terrain

2026-10-04. The client keeps no terrain heights after the build (the
world drops its `Terrain`), and a GPU depth read-back would need a hook
in `render/`. A tap or click on the overview (and the pad's A, the middle
of the screen) is cast onto the plane at the nearest road's height,
refined twice with the road height nearest the hit. Seen from straight
above the point's x and z are exact whatever the ground's height; its
height is the road's, so the orbit it flies to (350 m out, 34° down) can
sit a little above or below the hill there. The orbit's centre can be
moved with the keys or the stick (Q/E down and up).

## D885. Fog off, the far plane out, and the near plane

2026-10-04. Fog off sets `FogExp2`'s density to 0 after `Sky.update` (the
colour kept), outside `render/`. The far plane extended is 300 km instead
of the game's 9 km; Bevy's perspective is infinite reversed-z, so the far
plane only culls, and nothing in the shaders depends on it (the dome is
drawn at the far plane by its own shader). From high up the near plane
moves out to a thousandth of the height above the road (at most 50 m;
0.3 m, the game's, below 300 m), for depth precision over the whole
level. The overview turns fog off and the far plane out on the way in
and gives back what they were on the way out, unless they were changed
meanwhile.

## D886. Scene groups

2026-10-04. The panel's groups are the level's top-level scene groups:
the children of the world's root (`world:<level>`: terrain, road, each
scenery module's group, the sea, …) and the other drawn roots (the sky
dome), each named by its node, or by the material kind it draws when
unnamed. `animate::SceneIndex` records them when the loader indexes the
scene (`viewer::groups::scene_groups`, a pass over the nodes). Hiding a
group puts its entities on a render layer no camera or light draws
(`RenderLayers::layer(31)`), so the animators' own visibility edits are
left alone and showing it again restores exactly what they say. The link
names hidden groups (`hide=terrain,sea`).

## D887. The panel and overlay

2026-10-04. The panel is Bevy UI through the widget module's tokens, type
and text (`ui::widgets`), top left, folding to its header (folded at
first on a touch screen, open on a desktop; `panel=0` in a link). It is
rebuilt only when its shape changes (a camera, a toggle, a group, a
note); the sliders, the route and time-of-day labels and the readout
(four times a second) change in place. The overview's route (240
segments) and its zone marks and names are UI nodes built once and moved
by `UiTransform` only (no layout), and only when the overview's pose
changes. On a touch screen the move stick (bottom left) and the ↑ ↓ rise
and sink buttons (bottom right) are drawn and hit-tested by the viewer.
The viewer renders at the race's pixel ratio (`hq ? min(dpr, 1.5) : 1`,
not the menus' sharper one, D575), so its frames cost what the race's do.

## D888. The link and the address

2026-10-04. The link is the view's query string (`viewer/link.rs`):
`view=god&level=…&mode=…&cam=…`, the orbit's centre, the ride's `s h back
lat v yaw pitch`, `fog far anim`, `t`, `hide`, `speed`, `panel`, positions
to the centimetre and angles to 10⁻⁵ rad. The page's address follows the
view (`history.replaceState`, once the camera has been still for half a
second), so the address bar is always the view's link; Copy link also
puts the full URL on the clipboard, inside the tap through the page's
gesture bridge (`viewer_gesture`, where Safari allows it) and from the
frame otherwise. Natively the link is printed as a `--query` argument.

## D889. Draw calls and triangles for the readout

2026-10-04 (frame-time agreed it lives in `viewer/`). Counted in the
render world after the queues are built (`viewer/stats.rs`): every mesh
entity visible from the camera, and from the sun's shadow map, is a call,
its triangles the index count over three times its instances (an
`InstancedMesh`'s count from `render::instancing::Instances`, read only).
That is how three's `renderer.info.render` counts, the unit of SPEC 6.6's
budgets; Bevy may batch some of these into fewer GPU draws, and the post
chain's passes are not counted. Counted only while the panel is open (or
`__mp.viewer.set({count: true})`), with no allocation per frame.

## D890. Pads in the viewer

2026-10-04. The viewer has no race, so it adds the pads' poll alone
(`gamepad_io::pads_only`: `PadsRes`, polled in `PreUpdate`, WP 6.4's
maps and `__mp.pads` as before) and reads the active pad's standard
layout directly, with the race's dead zone and curve (0.12, ^1.4): left
stick moves, right stick looks (2.2 rad/s), LT and RT sink and rise, LB
and RB slow down and speed up (orbit and overview: zoom), Y the next
camera, X folds the panel, A in the overview dives to the middle, the
D-pad goes along the route (◂ ▸ 500 m) and moves the time of day
(▴ ▾), Start takes a screenshot.

## D891. The test bridge

2026-10-04. In a viewer run `__mp.screen` and `__mp.mode` are `viewer`,
`__mp.uiNodes` holds the panel's controls (`vw-…`, as the menus' ids),
and `__mp.viewer` reports the mode, pose, orbit, overview, ride, route
position and zone, toggles, groups, the link, frame time, draws and
triangles; `__mp.viewer.set({mode, cam, orbit, ride, fog, far, anim, t,
hide, speed, route, folded, help, count})` takes a pose and the panel's
state (with `cam`, at once, no flight). `tools/parity/e2e/viewer.test.mjs`
drives it; `tools/parity/viewer-shots.mjs` takes the review pictures and
the measurements into `parity/report/viewer/`.

## D892. Pointer units on the web, and the panel's scale

2026-10-04. With the race's pixel ratio forced (D887: 1 on a phone with
High quality off), winit hands touch and cursor positions in the device's
pixels over the overridden scale factor, not in the UI's logical px (the
race's controls convert with `touch_scale`, D846). The viewer measures
CSS px per logical px as the race does (the canvas's CSS width over the
window's logical width) for the panel's breakpoints and the bridge's
boxes, and takes pointer positions to logical px by `(scale /
base scale) / css` (`Viewer::ptr`); natively both are 1. On a phone the
panel stops above the touch stick and scrolls.

## D893. What the viewer costs

2026-10-04, release WebGPU build, headless Chrome on the RTX 3060, frame
rate uncapped, `tools/parity/viewer-shots.mjs --race` (desktop 1280 × 800,
High quality on) and `--device iphone` (844 × 390 at dpr 3, High quality
off, so no shadows and pixel ratio 1); the machine shared (load 23 to 72),
so the tails are other processes' as much as ours. Frame time p50 / p95 in
ms; draws are objects (camera + shadow map) and triangles in millions,
counted as D889; wasm is the memory's high-water mark (the same in every
mode: the level's build sets it).

| Level | Free | Orbit | Overview | Far plane out, fog off | Ride | Race (autopilot) | Draws: free / orbit / overview | M tris: free / orbit / overview | Wasm MB (race) |
|---|---|---|---|---|---|---|---|---|---|
| Sierra | 7.6 / 26.6 | 8.2 / 29.0 | 5.6 / 30.7 | 3.8 / 15.2 | 8.1 / 29.5 | 7.7 / 32.4 | 394 / 112 / 504 | 2.90 / 1.17 / 3.16 | 347 (370) |
| Coast | 4.7 / 20.6 | 4.6 / 23.0 | 5.9 / 28.8 | 6.6 / 31.0 | 4.7 / 22.9 | 6.0 / 23.1 | 329 / 99 / 428 | 2.56 / 1.28 / 3.24 | 456 to 504 (461 to 493) |
| Streets | 4.2 / 15.7 | 3.8 / 14.3 | 4.6 / 26.5 | 3.8 / 14.6 | 3.8 / 21.2 | 5.8 / 15.2 | 214 / 131 / 359 | 1.35 / 0.94 / 1.71 | 394 (398) |
| Desert | 6.7 / 15.1 | 4.4 / 13.5 | 9.9 / 26.8 | 3.8 / 24.1 | 5.0 / 21.7 | 5.9 / 27.6 | 308 / 139 / 349 | 2.39 / 1.40 / 2.68 | 310 (315) |
| Seaside | 5.1 / 21.4 | 4.1 / 9.5 | 4.9 / 10.6 | 4.0 / 12.1 | 8.4 / 32.0 | 4.6 / 14.7 | 125 / 129 / 166 | 1.09 / 1.10 / 1.06 | 210 (210) |
| Cruise | 4.5 / 13.3 | 6.1 / 13.5 | 7.5 / 27.2 | 4.2 / 12.2 | 3.8 / 34.8 | 4.5 / 15.8 | 198 / 208 / 357 | 1.72 / 1.85 / 2.30 | 438 (443) |

(the first desktop run; a second, at load 23 to 55, gave the same draws,
triangles and memory and frame times within its noise, Coast's wasm 456
MB against 504 in the first.) Free fly and orbit hold the race's budgets:
their median frames are the race's on the same build and machine (within
the run-to-run noise), and their draws and triangles are under the JS
figures plus 10 % (SPEC 6.6: 833, 705, 686, 644, 404, 721 calls; 3.48,
3.50, 1.98, 3.13, 1.18, 2.06 M triangles). The overview draws the most
(up to 504 objects, 3.2 M triangles; the Cruise's 2.30 M is over its
race figure, which SPEC 8.6 allows), but no shadow pass (the sun's box
follows the camera, high above anything), so its frames cost about what a
race's do. iPhone emulation (desktop GPU, so the frame times say little
about a phone; the memory is the phone's): the same wasm high-water marks
(206 to 488 MB), constant across modes, so no mode grows memory and
nothing needed degrading for the 512 MB budget; the overview at 172 to
484 objects and 1.1 to 3.2 M triangles is within the races' peaks on
every level but the Cruise. What stays near the budget is Coast's own
build (D678: 512 MB on reload; 456 to 504 here), not the viewer: a viewer
run holds no race field and no sound graph. Load to ready 4.6 to 13.8 s
(the level's build).

## The test bridge and the JS suites on the Rust build (WP 6.7)

Roadmap WP 6.7 and M6's exit ("the e2e suites are green against the Rust
build, desktop and phone emulation"), SPEC 8.5. Decisions D900 to D919.

## D900. The harness's `target` option: the JS suites unchanged

2026-10-04. `test/e2e/harness.js` gains `target` (`openGame(browser,
{ target })`, `launch({ target })`, default `MP_TARGET` or `'js'`). Under
`'js'` nothing changes: every new branch is behind `target === 'rust'`,
and the JS game's requests, flags and timing are as before (the
JS run after the change, `--target js`, is in D908). Under `'rust'`:
- Chrome gets the WebGPU flags of the Rust tools (`--enable-unsafe-webgpu
  --enable-features=Vulkan`, beside the harness's own) and a long protocol
  timeout; the working tree is answered by the same request interception,
  with `application/wasm` and `font/ttf` added for the Rust page only. The
  page is `dist/next/index.html` with the test's query. The 90 MB rule of
  `tools/parity/e2e/harness.mjs` (D577) is not carried over: since D678 a
  level is built by the client, and the page downloads no full export.
- `center(sel)`/`tap`/`click` look the selector up as a control id
  (`rust-bridge.js` `selectorId`: `#btn-start` is `btn-start`, `#level-pick
  .lvl-tab:nth-child(3)` is `lvl-tab-streets`, `#touch [data-tap="pause"]`
  is `touch-pause`, …), scroll it to the middle (`__mp.reveal`) and take
  the centre of `__mp.ui(id)`, failing as `elementFromPoint` did when the
  client's hit test finds another control on top.
- The suites' page code is not rewritten. `installBridge` (run before the
  page's scripts, on the Rust page only) defines `window.__race`, `__game`,
  `__audio`, `__world`, `__camera`, `__stats`, `__pads`, `__pursuit` and
  `__ready` as views over `window.__mp`, and `game.eval` runs each
  evaluation inside `__mpShim.run`, which for its duration answers
  `document.getElementById`, `querySelector(All)` and `getComputedStyle`
  for the selectors the suites use with stand-ins over `__mp.uiNodes`
  (the page's own scripts never see them). A view is built once per
  evaluation from the last frame's snapshots.
- A write to a view (`r.player.vx = …`, `r.phys.reset(s, lat)`,
  `a.writePos()`, a select's `value` and `change`, `el.focus()`) becomes a
  `__mp.stage` command, sent in order when the evaluation ends; the
  client counts what it applied (`__mp.staged`) and the harness waits for
  that frame before the next read, since the JS game applied writes at
  once. Likewise every mouse, touch and key event the harness or a suite
  sends (`page.mouse`, `page.touchscreen`, `page.keyboard`,
  `game.touch`) waits two frames on the Rust target: the JS handles an
  event in its handler, the client in its next frame.
- `snapshot()` reads `__mp` (`screen`, `mode`, `race.state`,
  `audio.context`, `touchUi`); `waitReady` waits for `__mp.ready` with
  the menu's controls laid out (`__ready`), up to four minutes.
- `music.html` (the `path` option, and the menu's Music player link) is
  the JS page, which the Rust menu links to (DEVIATIONS: "The music
  player link opens the JS page"); there the bridge is not installed and
  evaluations run as for the JS game.

`npm run test:e2e:rust` (`tools/parity/e2e/js-suites.mjs`) runs every
suite with `MP_TARGET=rust`, one Chrome at a time, waiting before each
suite for the GPU's memory to be under 9 GB (the machine is shared), and
prints the suite × device table with the reason for every skip (D908).
`--target js` runs the same table for the JS game; `--only` takes a name
pattern. A suite skipped whole is not started (its `before` would launch
a Chrome that no `after` closes).

`test/` is outside the JS-tree key (`tools/parity/lib/jstree.mjs` hashes
`src`, `vendor`, `index.html` and the kernel), so these edits change no
cached parity data.

## D901. `window.__mp`, complete for the suites

2026-10-04, SPEC 8.5. What the client publishes, each frame unless said:
`ready`, `state`, `screen`, `mode`, `races`, `focus`, `touchUi`,
`uiNodes` (when it changes) and `ui(id)` (`{x, y, w, h, visible,
enabled, value, sel, z}`, CSS px), `reveal(id)`, `stage(cmd)` and
`staged`, and these snapshots:
- `race`: D846's fields plus `x, y, z, along` (the car's speed along its
  heading, `player.speed`), `prog`, `skid`, `damage`, `lap`, `lapTimes`,
  `playerFinished`, `playerTime`, `dist`, `lastS`, `odo`, `cruise`,
  `score`, `nearMisses`, `pursuitOn`, `traffic`, `place` and `racers`
  (`standings()`), `ais` (each rival's `s, lat, speed, prog, finished,
  finishTime, x, y, z, vx, vz, yaw`), `track` (`startS, finishS, n,
  length, loop, roadEnd`), `camPos`, `input.lookBack`, `touch.u` (the
  slider thumb's height); with the road itself as `trackFrame(s)` (`[x, y,
  z, fx, fz, rx, rz, hw]`) and `trackWrap(s)`, read at once.
- `audio`: as before plus `musicOn`, `musicGate` (`musicGate.gain.value`,
  `GameAudio::music_gate`) and `vol` (`_vol`).
- `hud`: `shown`, `laps` (`#hud-lap`), `texts` (`#hud-lap-n` is
  `texts.lapN`, …), from `HudState::bridge`.
- `lamps`: `{n, lit}`, the countdown lamps the last countdown frame set
  (Seaside's start lights, `scenery[0].lampMats`; D682's open item).
- `settings` (the menu's options, for a control not on screen) and
  `selects` (each drop-down's values, a `<select>`'s `options`).
- `pads`, `padsetup`, `padNav` (WP 6.4); `pursuit` is `{available:
  false, waits: 'roadmap M8 (Hot Pursuit)'}` until M8 (D906).

SPEC 8.5 writes `race()`, `pursuit()`, `audio()`, `stats()`; they are
properties holding the last frame's snapshot, as the tools since WP 2.5
read them (`__mp.race.state`), not functions. `stats` is `{fps}` on the
test side: frames counted between two reads of `__mp.frames`, so the page
adds no per-frame work (the frame-time package times the frame).

`stage(cmd)` takes, beside D577's and D846's (`finish`, `cruise`,
`padsetup`, `place`, `nitro`, `autogas`): `reset {s, lat}`
(`phys.reset`), `set {path, value}` (`player.x|z|vx|vz|yaw|s|lat|speed|
prog`, `phys.nitro|damage`, `lastS`, `odo`, `score`, `nearMisses`,
`cam.snap`, `touch.autoGas`, `progS.<i>`, `ais.<i>.s|lat|speed|prog|
finished|finishTime`), `aiWritePos {i}`, `event {type: 'impact', …}` (a
wall impact added to the next frame's events, `flow::Race::inject`, for
the rumble), `choose {id, value}` (a drop-down's choice), `slide {id,
value}` (a range's value, 0 to 100), `focus {id}`, `act {id}` (a click
by id), and `audio {op: 'suspend' | 'resume'}` (the context, as iOS
interrupts it). Each frame's commands are applied before its ticks, and
the race's previous state is set to the staged one (no interpolation
across the jump).

## D902. Every JS read, and what it maps to

2026-10-04. The suites' reads of the JS game, through `rust-bridge.js`:

| JS | Rust |
|---|---|
| `__ready` | `__mp.ready`, not on the loading screen, controls laid out |
| `__game.mode` | `__mp.mode` |
| `__race.state, time, countdown, cruise, lap, lapTimes, playerFinished, playerTime, dist, score, nearMisses, lastS, odo, pursuitOn` | `__mp.race.*` (the last race kept after Main menu, as `window.__race` is) |
| `__race.player.{x, y, z, vx, vz, yaw, s, lat, speed, steerAngle, prog}` | `race.{x, y, z, vx, vz, yaw, s, lat, along, steerAngle, prog}`; writes: `set` |
| `__race.phys.{locked, gear, nitro, nitroActive, skid, damage}`, `.reset(s, lat)`, `.events.push(impact)` | `race.*`; `reset`; `event` |
| `__race.track.{startS, finishS, n, length, loop}`, `.frame(s)`, `.wrap(s)` | `race.track.*`, `__mp.trackFrame`, `__mp.trackWrap` |
| `__race.ais[i]` (`s, lat, speed, prog, finished, finishTime`, `.v.*`, `.writePos()`) | `race.ais[i]`; `set`, `aiWritePos` |
| `__race.input.state.*`, `.input.touch.{stickR, stick, steering, held, tilt.state, autoGas}` | `race.input`, `race.touch`; `autoGas`: `set` |
| `__race.cam.mode`, `.cam.snap = true` | `race.camMode`; `set cam.snap` |
| `__race.progS.set(car, s)`, `.standings()`, `.traffic` | `set progS.<i>`; `race.place`, `race.racers`; `race.traffic` |
| `__race.__tagged` (flow-helpers' `markRace`) | `__mp.races` at the time of the mark |
| `__race.pv`, `.playerBody`, `__pursuit` | none (M8) |
| `__audio.ctx.state`, `.ctx.suspend()`, `.ready`, `._musicOn`, `.musicGate.gain.value`, `._vol`, `.trackInfo.id` | `__mp.audio.context` (`none`: no `ctx`), `audio` stage, `ready`, `musicOn`, `musicGate`, `vol`, `playing` |
| `__audio._radioCur` | none (M8) |
| `__world.level.id` | `__mp.level` |
| `__world.scenery[0].lampMats[].emissiveIntensity` | `__mp.lamps` |
| `__world.renderer.compileAsync` | none (the race-button double tap, D908) |
| `__camera.matrixWorld.elements[0, 2]`, `.position` | `race.camRight`, `race.camPos` |
| `__stats.fps` | `__mp.frames` between reads |
| `__pads` | `__mp.pads` |
| `#loading, #menu, #pause, #results, #padsetup` `.hidden` | `__mp.screen` |
| `#hud`, `#hud-lap`, `#hud-lap-n`, `#hud-lap-best`, `#hud-pen` | `__mp.hud` |
| `#touch` `.hidden` | `touchUi`, `race.touch.visible`, no screen up |
| `#touch [data-act=x]`, `[data-tap=x]`, `.t-slider`, `.t-drift-strip` | `ui('touch-x')`, `touch-slider`, `touch-drift` |
| `.t-stick` (`.active`, `.lock`, `style.left/top`), `.t-stick-knob` `style.transform`, `.t-pedal` (classes, `--u`, `--gas`, `--brk`), `.t-wheel` `style.transform` | `race.touch.stick`, `lock`, `knob`, `panel`, `u` (the JS's `SLIDER` bands), `wheel`, serialised as the CSSOM does |
| Controls by id (`btn-*`, `opt-*`, `lvl-*`, `res-title`, `res-best`, `pad-*`, `link-music`, `rotate-hint`, `tilt-note`, `np-pause`, `mode-pick`): `textContent`, `checked`, `value`, `.sel`, boxes, `closest('label')` | `ui(id)` (`value`, `sel`, the box; the control is its own label); not on screen: `__mp.settings` |
| `#level-pick .lvl-tab`, `#car-pick .pick` (`:nth-child(n)`, `:first-child`, `:last-child`, `.sel`) | `lvl-tab-<level>`, `pick-<car>` |
| `<select>` `.value = v` + `change`, `.options`; `<input type=range>` `.value` + `input` | `choose`, `__mp.selects`; `slide` |
| `el.focus()`, `el.click()` | `focus`, `act` |
| `#res-table tr` (`rowIndex`, `.me`, `cells`), `#res-extra .res-stat small` | `res-table` (rows), `res-row-<i>` (`place|name|value`, `sel`), `res-stat-<i>` (`value|label`) |
| `.pad-focus` (`id`, text, `.pad-edit`), `.pad-bind[data-act=x]` (`.on`, `.listening`, `b`), `#pad-name` | `__mp.focus` with `padNav`; `__mp.padsetup` (row labels: `Gamepad.js` `ACTIONS`) |
| `#mode-pick .sel` `dataset.mode`, `#mode-pick [data-mode=x]` | `mode-race`, `mode-pursuit` |
| `#pause .title` | `pause-title` |
| `body.touch`, `body.pad` | `__mp.touchUi`, `__mp.pads.connected` (set on the real body) |
| `#hud-pz`, `#hud-dmg`, `#pz-bar`, `#hud-hold`, `#pz-stars`, `#hud-radio-text`, the radio clips' resource entries | never shown (M8) |

## D903. `?level=` picks the menu's level, as in the JS

2026-10-04. Running the JS `circuit` suite showed the Rust build racing
at once on `?level=seaside`, which D432 decided before there was a menu
("no menu until M6"); the JS opens the menu on that level (`if
(params.has('level')) settings.level = params.get('level')`, not saved)
and only `autostart` races. `ui::menu_first` now follows the JS:
`?level=` sets the menu's level, and a run races at once with
`autostart=`, or with the Rust-only `race=1`, `car=` (D432) and the
native `shots=`. Every Rust tool that loads a level to race passes
`autostart`, `car=` or fly parameters already, but for
`tools/parity/e2e/phone.cjs`, which gains `race=1`.

## D904. Two fixes the suites found: `#np-pause`, `navigator.audioSession`

2026-10-04.
- The pause screen had no now-playing line (`#np-pause`, `showNowPlaying`:
  `♪ title · style`, set on every track change). `UiState::now_playing`
  follows the sound's `track_info()` in `sync_audio`; the pause screen
  draws it before Next track (the ♪ as the note icon, D573), and its
  control's value is the JS text.
- The client read `navigator.audioSession` once at start-up; the JS's
  `askForPlayback(navigator)` reads it when the context is made (the
  first gesture). `mp_audio::session::web::NavigatorSession` looks it up
  at each use; on an iPhone, where it is there from the start, nothing
  changes.

## D905. What stays in `tools/parity/e2e/`

2026-10-04. With the JS suites running on the Rust build, the adapted
copies that only repeated them are gone: `race-flow`, `touch-controls`
and `analog-controls`, and the repeated tests of `menu`, `race-button`,
`gamepad` and `tilt`. What the JS suites do not cover stays, Rust only:
- `menu.test.mjs`: the volume sliders and the track picker reach the
  sound; M shows on the pause slider.
- `race-button.test.mjs`: a second tap while the race is starting starts
  one race (`__mp.races`; the JS version holds three's `compileAsync`).
- `gamepad.test.mjs`: driving with the triggers and the stick (dead zone
  and curve), a map saved for the pad's id, rumble from a real wall hit
  and the countdown, the Controller screen from pause.
- `tilt.test.mjs`: an iPhone asks for motion access inside the Race tap
  and the Tilt tap (D843); another tab pauses the race.
- `sections`, `built-levels`, `level-switch` (the menu's sections and the
  client-built levels, D676, D678), the phone tools (`phone.cjs`,
  `phone-menu.mjs`), and `harness.mjs` with `controls-helpers.mjs` for
  them and the picture tools (`touch-shots`, `ui-shots`).

## D906. What waits for Hot Pursuit (M8)

2026-10-04. Six of the seven `pursuit` tests need the pursuit's client
(police on the road, the heat stars and bust bar, the radio, busts and
wrecks, pursuit results): `js-suites.mjs` skips them marked M8. The
bridge for them is listed so M8 can fill it: `window.__pursuit`
(`state`, `units[]` with `active`, `mode`, `siren`, `s`, `callsign`,
`v.model.root.visible`; `bust`; `player.hold`, `holdReason`,
`holdTotal`, `grace`; `busts`; `activate(unit, s, lat, speed, mode)`),
`__race.pv` (`say(line, now)`, `hurt(x)`, `penalty`, `damage`,
`wrecks`), `__race.playerBody`, `__audio._radioCur` (`srcs`), the DOM
`#hud-pz`, `#pz-stars`, `#pz-bar`, `#hud-dmg`, `#hud-hold`, `#hud-pen`,
`#hud-radio-text`, `#res-extra`'s pursuit tiles, and the radio clips'
`performance` resource entries. Until then `__mp.pursuit` says
`{available: false, waits: 'roadmap M8 (Hot Pursuit)'}`, the stand-ins
for those elements are never shown, and a pursuit staging command
(`pursuit`, `unit`, `roadblock`, `spikes`, `hurt`, `say`) logs that it
waits for M8. The seventh test (race mode: no police, no pursuit HUD)
runs and passes.

## D907. What the Rust build still shows the suites

2026-10-04.
- **A console warning on the menu** (owner: the widgets, WP 6.1; Bevy
  UI). Chrome warns once, as the menu first draws, `Calling
  [RenderPassEncoder "ui"].Draw with an index count of 0 is unusual.`
  (with `?sections=0` too, not in a race: `autostart` and `hud=0` runs
  have none). In `bevy_ui_render` (0.19.1, `prepare_uinodes`) a new batch
  starts at an empty index range and stays empty when its nodes add no
  quads (a clipped node, an empty text), and it is drawn anyway. The
  `audio` suite's "a few seconds of hard driving log no warnings or
  errors" fails on it (its `game.warnings` count from the page's load).
  Not fixed here: it needs a patch to Bevy's batching or finding the
  menu's node that adds no quads. Now and then the page also logs `A
  valid external Instance reference no longer exists.` at start-up
  (owner: the page, WP 2.1/2.7: its WebGPU probe's adapter).
- An `autostart` race with no gesture logs Chrome's "The AudioContext
  was not allowed to start" on every note the music schedules; the JS
  game does the same (its context is made outside a gesture too), and no
  suite counts warnings there.
- Fixed here: `?level=` (D903), `#np-pause` and `navigator.audioSession`
  (D904).

## D908. The JS suites on both builds

2026-10-04, release build, this machine (RTX 3060, headless Chrome, one
Chrome at a time). `node tools/parity/e2e/js-suites.mjs` (`npm run
test:e2e:rust`) and `--target js`. The device is the one each test opens:
the suites name it ("phone", "phonePortrait"), the rest are desktop
(1280 × 800).

| Suite | Device | JS | Rust | Rust: why not |
|---|---|---|---|---|
| analog-controls | phone | 5 pass | 5 pass | |
| audio | desktop | 6 pass | 5 pass, 1 fail | the menu's UI draw warning (D907) |
| audio | phone | 4 pass | 3 pass, 1 skip | deviation: a tap on no control of the pause screen resumes (DEVIATIONS.md, D578); pausing's suspend and Resume are covered by `race-flow` |
| circuit | desktop | 4 pass | 4 pass | |
| gamepad | desktop | 5 pass | 5 pass | |
| keyboard | desktop | 6 pass | 6 pass | |
| levels | desktop | 6 pass | 6 pass | |
| menu | desktop | 5 pass | 5 pass | |
| menu | phone | 4 pass | 4 pass | |
| music-player | desktop | 4 pass | 4 pass | the JS page, which the Rust menu links to (DEVIATIONS.md) |
| music-player | phone | 7 pass | 7 pass | the first test goes there from the Rust menu |
| pursuit | desktop | 7 pass | 1 pass, 6 skip | M8 (D906) |
| race-button | desktop | 2 pass | 2 pass | |
| race-button | phone | 5 pass | 4 pass, 1 skip | the double tap holds three's `compileAsync` and traps `window.__race`; `tools/parity/e2e/race-button.test.mjs` checks it with `__mp.races` (passes) |
| race-flow | desktop | 8 pass | 8 pass | |
| race-flow | phone | 3 pass | 3 pass | |
| tilt | desktop | 1 pass | 1 pass | |
| tilt | phone | 4 pass | 4 pass | |
| touch-controls | desktop | 1 pass | 1 pass | |
| touch-controls | phone | 9 pass | 9 pass | |
| traffic-lod | desktop | 2 pass | 2 skip | three.js internals (`CarModel.setFar` and its geometry groups, `Traffic.activate`, `.lod`); the Rust far-model switch (95 m out, 85 m in, the JS's hysteresis) sits inside the draw system, with nothing to bridge |

In all: the JS game 98 pass; the Rust build 87 pass, 1 fail (D907) and
10 skip (6 for M8, 1 deviation, 1 JS-only hook, 2 three.js internals) of
98. The Rust-only
suites left in `tools/parity/e2e/` (D905) pass: `menu` 1, `race-button`
1, `gamepad` 4, `tilt` 2, `level-switch` 2, `sections` 1. The full Rust
run takes about 16 minutes (each test loads the wasm and builds its
level), the JS one about 10, one suite at a time.

## Hot Pursuit on screen: the police, their lights and the props (WP 8.1, 8.2)

## D920. The police in the client: the field's graph, then PursuitView.sync per frame

2026-10-05, WP 8.1. PursuitView's `makeUnit` builds a unit's model as
`MODELS[type]` (the patrol car `police`, the interceptor `muscle` in
`livery: 'police'`, the SUV `policeSuv`) with `{ lod: 'low', far: true,
seed }`, seeds from 300 in the Pursuit's order (the seven units, then the
five roadblock cars), every mesh casting a shadow, and a sawhorse as
`sawhorseModel()`. `play::police::extras` lists them from the pursuit in
the simulation and `models::spawn_field` builds them into the field's graph
after the traffic, so they share the kit's materials and geometry as the
JS page shares its caches (D410); they are drawn and taken away with the
race's cars (`RaceCar`). The JS models a car built before its race starts;
the client builds them with the field, at the race's start, behind the
loading screen, and their pipelines join the warm-up (D458).

`play::police` is PursuitView.sync's visual half, a system after the
field is drawn and before the effects, as `Race.update` calls `pv.sync`
after its own visual sync and before `effects.update`. For each police car
(units, then roadblock cars) that is active: `Vehicle.sync` (its pose
between the last two ticks as every car is drawn, SPEC 6.5; the body
springs, the wheels' spin, the steer pivots, `setBrake`), `farLod` from
the camera (the camera of this frame, as the client's traffic uses it,
D440; the JS reads last frame's), `setHeadlights(max(0.15, lightsOn))`,
`setSiren(mode, t)` and the glow's daylight dimming; then each active
sawhorse's sync. `t` is PursuitView's clock (`this.t`, which the
simulation keeps per tick, D61) between the two ticks. `mode` is
`flash` turned to `steady` when the "Police lights flash" option is off;
the option is the menu's (`mr.flash`), read each frame as `race.pv.flash`
follows the menu's checkbox.

## D921. The siren glow billboards: `Patch::PoliceGlow`, two light slots a car

2026-10-05, WP 8.1. `glowMaterial` is a `ShaderMaterial` (the
`PoliceGlow` kind, hidden until now: `convert::stand_in`). It is ported as
a block of `three_material.wgsl` (`PATCH_POLICEGLOW`): the vertex shader
pulls each quad's centre towards the camera (`max(0.05, d − min(1 + d² ×
0.0015, d/2)) / d`), opens the corner in clip space by half `aSize`,
never below `uMin` (0.02) of the view's height, and passes vFar; the
fragment raises the glow texture's alpha to `2 − vFar` and gives `mix(uRed,
uBlue, vBlue) × a × (1 + 1.5 vFar)`, additive. Its `tonemapping_fragment`
and `colorspace_fragment` do nothing into the post chain's render target
(three renders into one there too), so the block writes linear colour as
the rest. The corner, `aBlue` and `aSize` ride in the extra attribute
(`models::glow_mesh`; `play::models` builds that mesh itself, so
`convert` is unchanged).

The uniforms change every frame for every unit: `setSiren` sets `uRed =
(2.4, 0.12, 0.05) × red` and `uBlue = (0.1, 0.35, 3.2) × blue`, and
PursuitView multiplies both by `0.3 + 0.7 × night`. A material edited per
frame is prepared again (D456), so each car's glow material takes two
material light slots the first time it is set (`Cars::set_glow`), holds
their indices in `kind0.xy`, and the shader multiplies `setSiren`'s
colours by them; `kind0.z` is `uMin`. The glow is culled by its geometry's
bounding sphere grown by the largest spot (`glowGeometry`) as an `Aabb`,
as three culls it, has renderOrder 2 (D810), and casts no shadow: PursuitView
sets `castShadow` on it too, but three's shadow pass draws the quads with
its depth material, where all four corners of a quad sit on one point, so
nothing of them reaches the shadow map.

## D922. The shared police light: the one point light, after the anchors move

2026-10-05, WP 8.1. `new THREE.PointLight(0xff2030, 0, 38, 1.6)` exists
only when High quality and the flash option are on as the race is made
(`if (hq && flash)`): neither turning on later makes one. Each frame it
goes to the nearest flashing unit within 40 m of the player: colour
`(r/k, 0.08, b/k)` with `k = (r + b) || 0.001` from that unit's
`sirenColor()`, intensity `(r + b) × 70 × (0.25 + 0.75 night) × (1 −
smoothstep(25, 40, d))`, at its siren anchor's world position plus 0.6 m
(the car's plus 1.8 m without an anchor); with none near, intensity 0. No
level has a point light of its own (the JS's only `PointLight` is this
one), so it is `Lighting::point`, which the shaders already light with as
three does (the point light first, then the spots). It is placed in
`PostUpdate` after the transforms propagate (`police::light`, as the
headlight spot, D760), since the anchor rides the body's springs.

## D923. The test bridge's view of the police

2026-10-05, WP 8.1, for WP 8.3's bridge (`__pursuit.units[i].v.model.root.visible`).
`crate::play::police::PoliceDrawn(pub Vec<bool>)` is a resource holding,
per `pursuit.units` index, whether that unit's model is shown this frame
(set by the same code that shows or hides it); empty outside a pursuit
race. The staged scenes (D930) fill it too.

## D930. L4: the staged pursuit scenes

2026-10-05, WP 8.1 and 8.2's gate. `parity/golden/pursuit/scenes.json`
holds ten scenes at fly-camera stations, the scenery frozen: three units
on the player's tail at blue hour seen from ahead, in a red burst
(`units-red`) and a blue one (`units-blue`), and with the flash option off
(`steady`: steady glows, no light); units 120 to 300 m off on their far
models with the glows at their minimum size (`far`); Sierra by day (`day`:
glows dimmed, the light at a quarter); a heavy roadblock from behind the
player (`roadblock`) and up close (`barrier-close`: the sawhorses' boards);
a spike strip with the unit that laid it and a third of a second of the
spiked rims' sparks (`spikes`), the strip up close (`spikes-close`); and a
second of smoke from a 95 % damaged player, a disabled unit and a
u-turn's two puffs (`damage`).

`tools/parity/pursuit-scenes.mjs --side js` draws each with the JS's own
`PursuitView.js` in the game's page (kernel on): a stand-in race (the
level's track and level, a player `Vehicle` with its model, an `Effects`
with the player added first, the pursuit and police streams of
`src/parity/sim.js` seeded with the scene's seed), a new `PursuitView` on
it (which adds its cars to the effects), the units activated with
`pursuit.activate` and their sirens set, `placeRoadblock` / `placeSpikes`,
then per frame with `Math.random = mulberry32(seed)`: PursuitView's clock,
the events due, `player.sync`, `pv.sync`, `effects.update`. `--side rust`
loads the web build with `?pv=<scene>` (`play::pv_stage`), which makes the
same pursuit from `mp_sim` (`Pursuit::new` on the same streams,
`activate`, `place_roadblock`, `place_spikes`), the same cars, and runs the
same frames through `play::police` and `play::fx` once the entities exist;
`__mp.pvStaged` counts the frames since. The pursuit is bit-identical
(WP 1.6), so the cars stand in the same places and the roadblock's gap is
where the JS put it; the effects draw from the scene's stream in the JS's
order (D801), so the smoke and sparks land on the same pixels.

At 1280 × 800 against the JS (`cargo xtask parity shots`), all within SPEC
12's limits, WebGPU and WebGL2 the same to the second decimal (WebGL2's
`units-blue` block 0.327):

| Scene | mean ΔE00 | 95 % block |
|---|---|---|
| units-red | 0.161 | 0.343 |
| units-blue | 0.161 | 0.329 |
| steady | 0.163 | 0.323 |
| far | 0.140 | 0.337 |
| day | 0.211 | 0.494 |
| roadblock | 0.156 | 0.400 |
| barrier-close | 0.134 | 0.331 |
| spikes | 0.181 | 0.413 |
| spikes-close | 0.173 | 0.327 |
| damage | 0.151 | 0.357 |

What is left is edges and the player's own headlight lenses (the day
scene's largest block). `barrier-close` was 0.485 before D945: the JS
draws the roadblock SUVs' exhaust flames. Pictures:
`parity/report/pursuit/scenes/{js,rust,rust-gl}/` and the comparison
`parity/report/shots-pursuit/` (not in git; on the registered server).

## D931. The pursuit in a race, beside the JS

2026-10-05. `tools/parity/pursuit-race.mjs` (after `race-night.mjs`)
pictures the JS game (`?parity=1`, stopped at the tick) and the Rust build
(its own screenshot the first frame the race reaches the time) in the same
seeded pursuit race with the autopilot: Coast, seed 1, heat 3. The
simulations agree (the units' callsigns, modes and s at 40 s match `mp-sim
race --state-at`), and the pictures show the same cars in the same
places: a unit oncoming beside the player at 40 s, a disabled unit
smoking at the player's side after a takedown at 24 s, the wrecked
player's white smoke at 74 s. The Rust picture is one tick after the JS
one, which is enough to change a siren's burst (each lasts 55 ms of
100): at 40 s the JS light is between bursts and the Rust one in a red
burst. The `1 pipeline(s) compiling after the warm-up` the page logs at
the picture's place is the screenshot's own pipeline (a race without
pursuit logs it where its picture is taken too). Pictures:
`parity/report/pursuit/race/`.

## D940. The props in `mp_worldgen`, and their L3 gate

2026-10-05, WP 8.2. `mp_worldgen::pursuit_props` ports PursuitView.js's
`sawhorseModel` (the 128 × 16 canvas of red and white diagonal bands drawn
with `mp_canvas`, a `CanvasTexture` in sRGB, the board and four A-frame
legs, `dims`) and `spikeStrip` (a base box and `round(w / 0.16)` pairs of
four-sided cones merged without an index, placed at `pointAt(s, (lat0 +
lat1) / 2)` turned to the road). The JS module keeps the sawhorse's parts
and the strip's material in module variables; `PropKit` holds them for one
graph. `tools/parity/pursuit-props-golden.mjs` writes
`parity/golden/car_model/props.json` from the models export's group
`pursuit-props` (D413's lines, written out whole: there are eight), and
`tests/pursuit_props.rs` builds the same: every node identical, 3 of 3
materials identical, the board's texture within WP 3.2's threshold (mean
absolute difference 0.12, 1.62, 1.73, 0.19 per channel against Chrome's:
the antialiasing of the diagonal edges).

## D941. Spike strips and sawhorses drawn

2026-10-05, WP 8.2. `syncSpikes`: when the pursuit's strip is not the one
drawn (by its s, lat0 and lat1, where the JS compares the object), the old
one goes and a new mesh is made where it lies (`spike_strip` into a small
graph, converted as the scene is), with the strip material made with the
field; a strip's geometry on that material joins the warm-up then, so
laying one compiles nothing. It receives shadows and casts none, as the
JS's. A sawhorse is drawn as `Vehicle.sync` draws it (no wheels, no
lights), its flight after a hit coming from the simulation (`Surface::Raised`
height in its y).

## D942. The pursuit's smoke and sparks

2026-10-05, WP 8.2. PursuitView.sync's calls into `Effects` are
`police::emit`, run by `play::fx` after the frame's own sparks and before
`effects.update`, in PursuitView's order: the events' (a u-turn's two
puffs at the player's height, a barrier hit's fourteen sparks 0.8 m up),
the disabled units' smoke (`Math.random() < 0.3`), the smoke from under a
damaged bonnet (over 0.45 damage, `< (damage − 0.35) × 1.2`, darker with
damage), the spiked rims' two sparks a side (over 5 m/s, `< 0.7`). They
draw from the effects' stream (D801), the once-a-frame chances at their
60 Hz rate (D802). A disabled unit smokes from its position at the tick.
The barrier's sparks take the player's velocity as the tick left it,
after the simulation's 3 % slowdown (`v.vx *= 0.97` follows `sparksAt` in
the JS; DEVIATIONS.md).

## D943. The police in the effects

2026-10-05, WP 8.2. `this.cars = [...units, ...blockCars]` are added to
the effects after the traffic (`race.effects.addCar(u.v)`): their
headlight pools (7 × 12 m, 7.5 m ahead, when shown and after dusk) and
their exhaust flames, with the cars as drawn between the ticks.

## D945. Cars without an extras entry show their flames (a JS quirk, kept)

2026-10-05, found by `barrier-close` (D930). `Effects.update` reads `ex =
extras.get(v) || {}`; `Race` gives entries to the player and the rivals
only, so for a traffic car or a police car `ex.nitro && visible` is
`undefined`, which `f.visible = nit` stores, and three skips an object only
when `visible === false`. Every traffic and police car's exhaust flames
are drawn, unstretched (length 1, the core inside), in the JS game: the
blue-white cones behind the roadblock's SUVs and behind traffic (a van at
Coast's 74 s in both games, D931). Port, don't improve: the client now
does the same (a car past the end of the extras has no entry and shows its
flames, no random draw), so traffic changes too. DEVIATIONS.md lists it
with the JS's quirks; making the flames show only with nitro is a
one-line fix on both sides and the owner's call.

## WP 8.3 decisions: the pursuit HUD, results and mode switch

Roadmap M8, WP 8.3 (`HUD.js`'s Hot Pursuit furniture, `hud.css`'s Hot
Pursuit rules, `PursuitView.events`' texts, `main.js`'s pursuit setup and
results). Decisions D960 to D979.

## D960. The mode switch, end to end

2026-10-05. What WP 6.2 built already holds: the level card's Race / Hot
Pursuit control saves `mr.mode.<level>`, `ui::arm` gives the race
`pursuit` from it (`?pursuit=1|0` forces it, levels without police never
have it), the start button says Hot Pursuit, and the results keep the
best time under `mr.best.<level>.pursuit`. The `pursuit: false` call sites
are not race starts: `cars.rs` is the attract loop's stand-in cars, and
`session.rs`, `flow.rs`, `camera.rs` and `pose.rs` have it in tests. The
pursuit suite's menu test (toggle, remembered per level, a pursuit from
the menu) and results test (the tiles, the `.pursuit` key) pass against
the Rust build. Two of `startRace`'s pursuit options were missing and are
added: `?cops=N` (`Number(params.get('cops'))`, 6 by default) and the
menu's flash option (`settings.flash`). `mp_sim::race::SimState::new`
makes the pursuit with 6 units and flashing lights, as the recordings do;
the client sets `pursuit.max_units` (`clamp(cops, 0, 6)`) and `flash` on
the new field and on every restart (`flow::PursuitOpts`,
`Race::set_pursuit_opts`), so the simulation's `RaceOpts`, its traces and
their hashes are unchanged. `holdControls()` (`PursuitView.js:80-84`)
was in the simulation already (`race::hold_controls`); the busted test
checks it (R does nothing, the throttle does nothing, the clock runs).

## D961. `PursuitView.events`: who shows what

2026-10-05. The JS's one switch is four parts in the port: the rules
(the release onto the road, the barrier's slowdown, a bust's crash) are
the simulation's (`race::pursuit_events`, WP 1.6); the centre pops,
toasts and the camera's jolts and snap are `play/hud/pursuit.rs` (this
package), called from `flow::Race::on_event` for each pursuit event with
the JS's texts, durations and classes (PURSUIT, ESCAPED, TAKEDOWN,
SPIKED!, BUSTED, WRECKED; SPOTTED, COOLDOWN — STAY OUT OF SIGHT, HEAT
LEVEL n, (HEAVY) ROADBLOCK AHEAD, SPIKE STRIP AHEAD, X HIT THE SPIKES,
SPIKES / ROADBLOCK DODGED, X BUSTED, BACK IN THE RACE; `cam.bump(1)`,
`bump(1.2)`, `cam.snap`); the rumble stays in `flow` (WP 6.4); the radio
lines, `say()` and its rate limit, and the sounds are WP 8.4's
(`play/radio.rs`); the smoke, sparks and spike strip are WP 8.2's.

## D962. The furniture's nodes, and what is approximated

2026-10-05. The nodes are built only in a pursuit race (the HUD's key
gains `pursuit`; the JS builds them hidden in every race, and
`setPursuit` shows them), in `index.html`'s order: `#hud-pen` under the
lap line, `#hud-pz` after the minimap, `#hud-dmg` in `.hud-br` (the
touch speedo box grows to 74 px and its nitro bar moves up to 16 px, as
`.hud-br.pz-on` does), `#hud-radio` and `#hud-hold` after the toast.
What each shows is the model's (`Hud::update_pursuit`, WP 6.3, with
`hud_in`'s `pursuit` now `pv.hudState()`: `Pursuit.hud(damage)` with
the served penalty, the units with the parked ones last as disabled, the
roadblock and spike marks). The sizes follow the CSS at its breakpoints
(`PzLay`: desktop, `max-width: 720px`, touch landscape and portrait,
`--inB` and `--thumbs-top` from the touch layout for the radio pill).
The heat stars are the HUD material's fourth kind (`hud.wgsl` `stars`):
the clip-path polygon as a signed distance, the `<i>` fill from the
left with its vertical gradient (`.max`'s too), `skewX(-8deg)` undone
per pixel, the `drop-shadow(0 2px 5px)` as the stars' alpha 2 px lower
over a ±5 px ramp, and the patrol opacity. The transitions and
animations run from the real clock like D824's: the stars' `opacity
.4s` (not while hidden, where none runs), the radio pill's `opacity
.3s, transform .3s` (slide up 10 px), `pzBlink .5s steps(1)` on the BUST
label with flashing on, `dmgPulse .6s ease-in-out alternate` on a
critical bar. Left out, as D822 and D573 leave such things: the hold
title's coloured glow (its dark shadow stays), the radio pill's and the
bust track's box shadows (Bevy draws a box shadow under a translucent
node), and the radio line's `text-overflow: ellipsis` (Bevy text has
none; the line is cut at the pill's edge). DEVIATIONS lists them.

## D963. The radio line from the lines said

2026-10-05. The HUD shows each line `race.radio.said` holds (the lines
past the rate limit this frame, `play/radio.rs`, WP 8.4's) with
`hud.radio(text, Math.max(3, text.length / 14))`, the length in UTF-16
units as the JS counts it. A paused race keeps last frame's list, so the
HUD takes a list once per race frame (`Race::frames`, counted after the
pause check). The bridge's `pv.say(line, now)` goes into
`Race::stage_say` and through the radio's own `say` in the next frame,
after the radio's new frame clears the list: the HUD text and the voice
agree, and the rate limit applies. (`play/radio.rs` is WP 8.4's, D980:
the event table and the gap run per tick; this package built against a
stub with the same names.)

## D964. The bridge for Hot Pursuit

2026-10-05, filling D906. `__mp.pursuit` is null without a pursuit (so
`window.__pursuit` is undefined and `__race.pv` null, as the JS clears
them) and otherwise `{available, state, heat, maxHeat, heatMeter, bust,
evade, busts, takedowns, flash, maxUnits, units[{active, mode, siren, s,
lat, speed, callsign, type, x, z, health, target, visible}], player{hold,
holdReason, holdTotal, grace, bust}, pv{damage, wrecks, penalty}}`.
`__mp.hud` gains `pz, pzBar, dmg, hold, pen, radio` (each shown as its
`.hidden` says), `radioText`, `pzLabel` and the stars' fills;
`rust-bridge.js` answers `#hud-pz`, `#pz-stars`, `#pz-bar`, `#pz-label`,
`#hud-dmg`, `#hud-hold`, `#hud-pen`, `#hud-radio` (`.show`) and
`#hud-radio-text` from them. New staging: `unit {i, s, lat, speed, mode,
dir}` (`__pursuit.activate`), `hurt {d}` (`pv.hurt`, through
`mp_sim::race::hurt_player`, an addition to the simulation's API: the
same `hurt` a tick runs), `say {text, parts, force}`, `roadblock {s}`,
`spikes {s}`, and `set` of `pursuit.state`,
`pursuit.units.<i>.speed|s|lat|target` (`race.playerBody` is the
player, a rival `rival:<i>`) and `pv.penalty|damage`. A unit's
`v.model.root.visible` waits for WP 8.1's `PoliceDrawn` (one bool per
unit from the police draw); until it merges the bridge says false.
`__audio._radioCur` and the radio clips' requests are WP 8.4's.

## D965. The pursuit suite, and the pictures

2026-10-05, release build, this machine. `js-suites.mjs` skips none of
the pursuit tests now. Before WP 8.4 merged (without WP 8.1 and 8.4): 5
pass —
the menu toggle, race mode (no police), busted (meter, BUST bar, hold,
R locked, held still, the clock runs, released ahead of the police, the
penalty under the clock), wrecked (damage bar, the wreck, repaired) and
results (tiles, `best.<level>.pursuit`); 2 wait: the police chase (all
but `active units are drawn`, which waits for `PoliceDrawn`) and dispatch
(the clip preload and `__audio._radioCur` are WP 8.4's; its HUD half,
`#hud-radio-text` after `pv.say`, was checked by hand). `hud-shots.mjs`
gains `--stage wreck --stage-at t` (at race time t the car is wrecked
and dispatch says a line, in both games) and lets `--query pursuit=1`
replace its default `pursuit=0`. Desert, seed 1, heat 2, beside the JS:
at 40 s the stars (two and a half), the EVADE bar and the damage bar;
at 31.5 s after a wreck at 30 s the WRECKED card with its seconds and
bar, the penalty under the clock, the critical damage bar, the
cooldown toast and the dispatch pill, on desktop, a phone held sideways
and upright: the same places, sizes and colours, but for D962's
approximations (and the radio's text, whose event lines are WP 8.4's).
With WP 8.4 merged (1995b38): 6 of 7 pass, dispatch included, and the
wreck picture's dispatch line is the JS's ("Suspect vehicle is totalled.
Tow it back onto the road."); the police chase waits only for WP 8.1's
`PoliceDrawn`. The whole Rust run before the merge: as D908's table but
for pursuit (5 pass, 2 waiting) and one `keyboard` failure that passed
when run again (the nitro on an empty tank, a timing flake on the busy
machine; nothing of this package's touches it).

## WP 8.4 decisions (Hot Pursuit's audio)

## D980. The radio half of PursuitView is `play::radio`, per tick

2026-10-05, WP 8.4. `PursuitView.js`'s radio (`radioT` and its four
seconds, `say(line, force)`, `zoneName()`, `heading()` and the table in
`events()` of what each pursuit event says and which stinger it plays) is
`crates/mp_game/src/play/radio.rs`. `flow::Race` gains `radio: Radio` and
`stage_say` (the test bridge's `race.pv.say(line, now)`, said at the next
frame), and `Race::frame` calls `radio.frame(dt)` after the pause check,
says the staged lines, and after the frame's event loop runs
`radio.events(&mut audio_ticks)`. A line that gets through goes into
`race.radio.said` (cleared each frame), which the HUD (WP 8.3) shows as
`hud.radio(text, max(3, len / 14))`, and into its tick's calls for the
voice. JS's `RADIO_GAP` is `SAY_GAP` here: `mp_audio`'s `RADIO_GAP` is
already the pause between a line's clips. The JS runs `events(dt)` once per
`Race.update`, and the parity drives update once per 1/120 s tick, so the
countdown of the gap and the events are per tick (each tick's pursuit
events ride on its `TickAudio`), not per frame: the lines, their gating and
their place among the audio calls are then the JS drive's exactly
(D981). `frame(dt)` only clears the frame's lines. The zone and heading are
read at the player's car after the tick, as `sync` reads them after the
update; the callsign of a unit an event names is `pursuit.police(i)`'s
(units, then the roadblock cars, as the events number them).

## D981. PursuitView's audio rides on `TickAudio`; the pursuit drive's calls are the JS's

2026-10-05, WP 8.4. `TickAudio` gains `pursuit: Option<PursuitTick>` (the
tick's pursuit events, the zone, heading, callsigns and state the radio
reads, and what `PursuitView.audio` reads: the units with their lights on,
not disabled, held or standing down, within 360 m, with their closing
speed and pattern, `hilo` searching, `yelp` under 60 m, else `wail`; the
mood, off while held, after the finish and on patrol; the damage; the
spiked tyres and the speed) and `pv: Vec<PvCall>` (`events()`' calls:
`sirenHorn`, `escaped`, `takedown(1)`, `spikePop`, the barrier's
`impact(0.35, 0)`, `busted`, `wrecked` and `radioLine(parts)`). The audio
replays a tick as `Race.update` does: the race's one-shots, then
PursuitView's calls (in `sync`), then `update`, the nitro, the rivals, then
`setSirens` (the three nearest, panned by the camera as the rivals are),
`setPursuitMood`, `setDamage`, `setSpikedTyres` while the sound is ready,
then the tunnel. A takedown by the player and a wreck shake the audio's
camera rig (`cam.bump(1)`, `cam.bump(1.2)`), which the pans hear. A race
with a pursuit fetches its radio clips after `startRace`'s calls
(`radioVoice.prefetch(radioClips({ zones, units, names }))`: the track's
zones, every unit's callsign, the rivals' names), and the last race's
pursuit is disposed of (`setSirens([])`, mood off, no damage, no spiked
tyres) there and on the way to the menu, as `race?.dispose()` does.
`play::audio::tests::the_scripted_pursuit_makes_the_js_calls` runs the
client's frame loop through the reference's `pursuit` drive (Sierra, the
sports car, seed 1, the autopilot, heat 2, 7200 ticks, two ticks a frame)
and requires all 43,421 calls of `drive-pursuit.jsonl.gz` line for line,
every argument bit for bit but the camera's pans (31,627 of them, within
7.8e-16): the prefetch's 66 clip ids, 7200 each of `setSirens`,
`setPursuitMood`, `setDamage`, `setSpikedTyres`, the nine radio lines, the
siren horn and the takedown. The same nine lines reach `radio.said`.

## D982. The pursuit drive in the browser

2026-10-05, WP 8.4. `tools/parity/rust-audio-race.mjs --drive pursuit`
runs the web release build through the same drive (`pursuit=1&heat=2`,
7200 ticks) as D516 runs the race: before the gesture the graph is built
on a suspended context whose clock stands at 0; a key starts it; then
the client's 43,421 calls match the JS drive's, the startRace sequence
with the prefetch (8 calls) included. The rivals' and the sirens' pans
agree to 7.8e-16; 158 of the car contacts' pans differ by up to 0.014,
D516's effect (which ticks share a rendered frame). The audio costs 0.23 ms
a frame on the main thread on average there.

## D983. `__audio._radioCur` gives the browser's own nodes

2026-10-05, WP 8.4. The `pursuit` suite reads the radio transmission's
sources off `__audio._radioCur.srcs` (`s.buffer.duration`, `s.loop`,
`s instanceof OscillatorNode`). On the web backend those are real Web
Audio nodes, so the client hands the page the nodes themselves, not
stand-ins: `GameAudio::radio_cur_srcs()` lists the transmission's sources
(the hiss, the clips or the burble's buzz and breath, in order),
`mp_audio::wa::web::js_node` returns the browser node behind a facade node
(a `Backend::js_node` that only the web backend answers), and
`play::audio` publishes `{ srcs }` on `__mp.audio.radioCur` (rebuilt only
when the transmission changes); `rust-bridge.js`'s `audioView()` returns
it as `_radioCur`. The radio clips are fetched from `../../audio/radio/`
relative to the page (D513), so the `performance` resource entries the
suite counts are the browser's own. With stand-ins for the parts of the
bridge that are WP 8.3's (`__race.pv.say`, the units' callsigns, the
radio text), the suite's "dispatch speaks in the recorded voice" test
passed on the Rust build: the level's clips preloaded, the intercept
call played as two clips of the right lengths with no burble, an
unrecorded line got the burble.

## WP 6.8 decisions: the native window, its keys, focus pause and the CI smoke test

## D1000. The player's window keeps its state, as the Electron shell's did

2026-10-05, WP 6.8. `native::window_state` ports `desktop/main.cjs`'s
window: the interactive window opens at the bounds saved in
`window-state.json`, `{x, y, width, height, fullscreen, maximized}` as
`saveState` writes it, maximised or fullscreen as it was; 1600 × 900 the
first time (a missing or zero size is the default, `st.width || 1600`),
800 × 450 at least, titled "Midnight Racer", with `desktop/icon.png` as its
icon (as every interactive window has, `--size` ones too). It is saved when the window is asked to close, on Ctrl+Q, and on any
other exit of the app while the window is there (the terminal's Ctrl+C
too), with the last bounds seen while the window was neither maximised nor
fullscreen when it is either (`win._lastBounds`).

- **Where.** Beside the settings store (`FileStore::default_path`'s
  directory, `~/.config/midnight-racer/` on Linux, the directory of
  `$MP_STORE` when that is set). Electron's own file
  (`~/.config/Midnight Racer/window-state.json`) is not read: the first
  native run opens at 1600 × 900.
- **Units.** The client draws at a scale factor of 1 natively, so the
  bounds are the window's pixels: `x`, `y` the outer position (what winit
  places), `width`, `height` the inner size. The minimum goes to winit as
  Bevy passes it, in desktop logical pixels; at a desktop scale of 1, as
  here, they are the same.
- **Which runs.** Only the player's window: not `--smoke-test`,
  `--screenshot`, `shots=`, `--stations`, `--materials`, nor any run with
  `--size`. Those keep exactly the window they had (1280 × 800, 512 × 512
  for the material scenes, `--size`, hidden for the pictures), so no picture
  changes; a unit test pins it.
- **Fullscreen and maximised, read back.** winit's `fullscreen()` is the
  last it asked for and misses the window manager's own changes, and
  `current_monitor()` is a cached guess. Where the position is known (X11)
  a window is fullscreen when it covers one of the monitors exactly; on
  Wayland, which tells no position, it is winit's state (the compositor's).
  A saved fullscreen opens on the monitor holding the saved position
  (Bevy's `MonitorSelection::Entity`); `Current` put it on the wrong one
  of two.
- **Platform notes.** Wayland gives no position, so none is written there
  and a saved one is kept. The X11 class and Wayland app id are
  `midnight-racer`, the desktop entry's `StartupWMClass`
  (`tools/install-desktop-entry.sh`), so the desktop claims the window with
  the entry's icon (the counterpart of Electron's `CHROME_DESKTOP`); on
  Wayland that is the icon shown. The icon itself is built in at 64 × 64:
  the 256 × 256 one left X11's `_NET_WM_ICON` empty (a quarter of a
  megabyte in one property request). `winit` is a direct dependency of
  `mp_game` for `winit::window::Icon` (Bevy 0.19 has no icon API), at the
  version Bevy uses, and `image` gains `png` (already on through Bevy's).

Checked on the dev machine (GNOME on Wayland, two monitors): on XWayland,
driven with `xdotool`/`wmctrl` configure requests and EWMH messages, a
first run at 1600 × 900; resized and moved, then maximised and closed, it
saved the normal bounds with `maximized: true`, reopened maximised, and
unmaximised to those bounds; fullscreen by the window manager saved as
fullscreen with the normal bounds, reopened fullscreen on the same monitor,
and left fullscreen by the window manager saved as windowed; `--size` and
`--smoke-test` left the file alone. On Wayland (the session's own backend),
the first run opened at 1600 × 900 and a saved maximised or fullscreen state
read back as it was saved.

## D1001. The window title is "Midnight Racer"; the status line with `stats=1`

2026-10-05, WP 6.8. The player's window is titled "Midnight Racer", as the
Electron window (`title: 'Midnight Racer'`), not the status line that
changed every half second (state, level, s, fps, worst frame). The status
line stays in the title with `stats=1`, the native counterpart of the web
build's stats panel (SPEC 8.4's `stats` hook), and in the runs that are not
the player's window (`--size`, the pictures), where it served development
before.

## D1002. Focus loss pauses a race natively; F11 and Ctrl+Q

2026-10-05, WP 6.8, SPEC 8.4. `native::focus_pause` is the counterpart of
`play::web::visibility` (`if (document.hidden && mode === 'race')
pause(true)`): when the window loses the focus (Bevy's `WindowFocused`),
a race in progress pauses through the same `Race::pause(true)`, before the
frame's ticks, and stays paused when the focus comes back, as the JS does
on returning to the page. The input layer already lets go of every key on
focus loss (`read_input`). It does not pause in the runs that make
pictures or check the build (their window may never have the focus) nor
with `autodrive=1`, where nobody drives and a measurement would stall: a
window loses the focus far more often than a page goes hidden.

F11 toggles borderless fullscreen (Electron's `setFullScreen` on Linux),
on the monitor under the window's middle, judged from the window's real
state (D1000) so a fullscreen the window manager made or ended is toggled
from what it is. Ctrl+Q and Cmd+Q (Super) quit, the chord read in the
order the keys came, since one slow frame can hold both the Ctrl's press
and its release. Neither reaches the game as a binding: `play::input::
dom_code` has no F11, and Ctrl+Q leaves before a Q matters. They are on for
every interactive window, `--size` included.

Checked on Xvfb, through lavapipe and XTEST: in a race on Seaside, F11
reached the window, another X client taking the focus paused the race (the
pause screen came up), and Ctrl+Q quit with exit code 0 and saved the
state. Not checked: F11 under a window manager by key. XWayland here
passes XTEST through the input-capture portal (`-enable-ei-portal`) and
Wayland has no injection, so the key was not pressed on the real desktop;
the same mode change, made at start for a saved fullscreen, was checked
there (D1000). Cmd+Q on macOS and Windows are untested.

Checked by the owner, 2026-10-05, on the real desktop (GNOME on Wayland,
`cargo run -p mp_game`, the menu): F11 by key went fullscreen ("F11:
fullscreen on"), Ctrl+Q quit, and `window-state.json` was saved as
`{"width":1600,"height":900,"fullscreen":true,"maximized":false}`: the
normal bounds under the fullscreen, and no position, as Wayland tells none.

## D1003. The native smoke test in CI: Xvfb and lavapipe

2026-10-05, WP 6.8, the gate. The `rust` job installs `xvfb`, `xauth`,
`mesa-vulkan-drivers` and `libvulkan1`, builds `midnight-racer` (debug, on
the dependencies `cargo test` built) and runs `--smoke-test --after 5`
twice under `xvfb-run` with `VK_ICD_FILENAMES` set to lavapipe's ICD and
`WGPU_BACKEND=vulkan`, with a store of its own (`MP_STORE`): the default
run (the menu's first screen, Sierra's section built in the client, D678,
D742) and a race on Seaside Raceway (`level=seaside&autostart=sports`),
the lightest level to build. Race timings through lavapipe under Xvfb on
the dev machine, to the end of the run: Seaside 11 s, Desert 18 s, Coast
21 s, Sierra 25 s, Streets 27 s, Cruise 32 s; the menu 7 s. Restricted
to four cores and without an ALSA device (`ALSA_CONFIG_PATH=/dev/null`),
as on a runner, the two CI runs took 18 s and 31 s and passed.

Locally the same way: Xvfb is not installed here, so the Ubuntu package
was unpacked into a scratch directory (`apt-get download xvfb`, `dpkg -x`)
and its `xvfb-run` used with `WAYLAND_DISPLAY` and `DISPLAY` unset;
lavapipe (Mesa 26.0.3, llvmpipe on LLVM 21) ran Bevy 0.19 without trouble.
The report names the adapter (`llvmpipe (...) (Vulkan, Cpu)`), so a run
on the real GPU cannot pass for one on lavapipe.

`--smoke-test` now follows the Electron one's check: a tracing layer beside
Bevy's (`native::log_tally`, under the same filter) counts the log's
warnings and errors; the report lists the first twenty, and any error fails
the run (exit 1), as a console error fails Electron's. Errors that say
nothing about the build count as warnings, as Electron filters the
favicon's 404: so far only web-audio-api's "buffer underrun or overrun",
which the sound's output stream logs while a software renderer holds a
frame up (it failed Desert, Coast and Streets on lavapipe before). The run
also fails on a failed load and when the scene is not up within 600 s
(Electron waits 60 s for `__ready`; lavapipe compiles slower than a GPU).
The report line is `smoke test: ok|failed: <level>, ready in <s>, <n>
frames after; adapter <name>; <e> errors, <w> warnings; <counts>`.

## D1004. `--query` and SPEC 8.4's debug hooks, natively

2026-10-05, WP 6.8. Every hook in SPEC 8.4's list reaches the native
client through `--query` (or bare `key=value` arguments), read by the same
code as the web's query string: `level`, `pursuit`, `heat`, `cops`, `t`,
the fly camera's `s`, `h`, `back`, `lat`, `yaw`, `pitch` (and `v`),
`timescale`, `autodrive` (also `--autodrive`), `autostart` and `touch`
(`touch=1` shows the touch controls, the mouse as one finger, D436). Gaps,
left as they are:

- `stats=1` natively is the window title's status line (D1001): frame rate
  and the worst frame, not the JS panel's draw calls, triangles, load time
  and heap. The web build's panel has the frame numbers and the adapter.
- The Electron shell's flags map as: `--url-query` → `--query`, `--shot
  <png>` → `--screenshot <png>` (its own run: with both, the picture is
  taken and the smoke report is not printed), `--wait <ms>` → `--after
  <frames>`. `--dev` (F12 devtools) has no counterpart.

## D1005. The menu's hitches natively: the views' work off the main thread

2026-10-05. The owner saw a stutter in the native menu (`cargo run -p
mp_game`, a debug build) and the log said `1 pipeline(s) compiling after
the warm-up`. `RUST_LOG=mp_game::status=debug` now logs every frame over
50 ms (`slow frame`), and a pipeline queued after the warm-up is logged by
its label (its shader defs at debug level). Measured on the RTX 3060, the
menu's first 15 s, the five other views built behind it:

- **What stuttered.** Not the late pipeline (natively pipelines compile in
  the background, so a late one draws late; it does not stall a frame) but
  each view's hand-over from its build thread: the main thread built the
  Track twice (before the build and after it) and `world_data` built it a
  third time for a level with a City or Harbor (Sierra, Coast, Cruise),
  then converted the textures one per step, Seaside's 1843 × 2160 photo
  with its mipmaps in a single step. Debug build: 8 frames over 50 ms, the
  worst 250 ms (the hand-over 21 to 225 ms, the photo 377 ms). Release
  build: a 74 ms hand-over (Cruise) and the photo's 39 ms step.
- **Now.** Natively the build thread makes the Track (with `runout`), the
  world build and the converted textures (`preview::build_on_thread`;
  `loader::Build::with_images` takes them); the main thread only adds the
  assets and spawns, 6 ms a frame as before. Debug build: no frame over
  50 ms in two runs (one of 51 ms in a third); release: spawns 3 to 19 ms,
  one frame of 51 ms in one run of two. The views are the same; the web
  path is unchanged (no threads there).
- **The late pipelines.** The menu's box shadow (`ui::widgets::shadow`)
  was compiled the first time the menu drew; on the web also Bevy UI's
  plain and gradient pipelines, because the page's loading screen draws no
  Bevy UI. `warmup::spawn_ui` adds an off-screen node with all three to the
  first scene's warm-up. A background view's own stand-ins
  (`SectionWarm`) queue pipelines by design; those no longer count as late
  (`preview::section_warming`). Natively the menu now has none late; the
  one left in a `--screenshot` run is the screenshot's own (D923).
- **Open, on the web.** The stutter while the views build behind the menu
  is D746's, accepted: 14 to 16 of about 83 frames over 50 ms, the worst
  167 to 183 ms (`sections.test.mjs`, load 7 to 10). The first tab switch
  (to Coast) has one frame of 283 to 317 ms, on main before this change
  too (D749 saw none; not yet found). With the labels, the web's log shows
  mesh pipelines (prepass, opaque, alpha-blend) queued after the warm-up
  behind the menu and in a race after it; each is a hitch in a browser.

## D1006. A keyboard that is also a joystick: the Keychron Q6 HE

2026-10-05, from the owner's native run. The Keychron Q6 HE (a Hall-effect
keyboard, USB 3434:0b60) exposes a joystick on its interface 0 beside the
keyboard (`/dev/input/event5` and `js0`, udev's `ID_INPUT_JOYSTICK=1`, 6
axes, 16 buttons). gilrs takes it for a gamepad and, having no mapping for
it (`No mapping found for UUID 03000000-3434-0000-600b-000011010000`), uses
its default one; it was connected first, so it is gamepad 0 and the Xbox
One pad gamepad 1. What follows, natively, as the pads work (WP 6.4, D781):

- The pads' state is `connected` with no controller in hand, so the race's
  stuck hint names a pad button and the Controller screen lists the
  keyboard.
- The active pad (the one the Controller screen and the hint describe) is
  the keyboard until another pad's button is pressed or stick moved; then
  that pad is, as in the JS.
- Every pad's input is summed (`Pads::poll`), so whatever the keyboard's
  joystick reports would steer or throttle; with its analog mode off it
  reports nothing, and the Xbox pad drives normally.

Nothing is filtered: a browser on Linux lists such a device in
`navigator.getGamepads()` too (Chrome takes `ID_INPUT_JOYSTICK` devices;
not checked with this keyboard), and the JS game would treat it the same.
Should it get in the way, the keyboard's own setting turns its joystick
off, or the port could skip pads that report themselves as keyboards; not
done.

## D1007. Natively, a click or key wakes the sound

2026-10-05. The owner heard nothing natively until a race had loaded.
`main.js` wakes the audio (`wakeAudio`: `init`, `unlock`) on every
pointer-down, pointer-up, touch-end, click and key-down; on the web the
page's handlers call `play::audio::gesture`, natively nothing did, so the
graph was built only by a race (`start_race`). The menu then stayed silent:
the buttons' clicks and a level tab's music (`if (audio.ready)
pickMusic()`) wait for the sound to be up. `play::audio::native_gesture`
(PreUpdate, any state) wakes it on a mouse button, a key or a touch ending.
As in the JS, the menu has no music of its own until a level tab (or the
track picker) asks for one; the first click builds the sound.

## D1008. `cargo xtask web` swaps the build in when it is done

2026-10-05. The owner, on the tailnet URL during a rebuild: a directory
listing instead of the game. `cargo xtask web` emptied `dist/next/` first
and wrote `index.html` last, minutes later. It now builds in
`dist/next.staging/` (with `--only`, a copy of the live build's other
backend first) and renames it into place when everything is written; a
failed build leaves `dist/next/` as it was. Checked: the URL polled every
0.3 s through a release build (694 requests) served the game every time.

## D1020. A debug overlay: `debug=1` and F3

2026-10-05, the owner's request: a panel over the game, on a flag or a
key, with the frame rate, sliding minimum and maximum over 5 s, 30 s and
all the time since it was turned on, small graphs of the frame times over
those spans, dark grey and nearly opaque with orange text and graphs.
`debug_overlay` (DEVIATIONS: the JS has only `?stats=1`).

- **Turning it on.** A new key, `debug=1` (natively also `--debug`), not
  `stats=1`: `stats=1` is SPEC 8.4's hook, already the native window
  title's status line (D1001) and the web page's HTML panel, and the
  measurement runs and pictures that pass it must not change. F3 shows and
  hides it in every run: nothing binds F3 (`play::input::dom_code` has no
  F keys, the menus read Tab, Enter, Space, Escape and P, the viewer no F
  key, the window F11), so the key reaches nothing else; on the web the
  canvas's key handler keeps the browser's own F3 (find) from it. A phone
  has no F3; there it is `?debug=1`. Turning it on starts the all-time span
  afresh.
- **What it measures.** Per frame, in ms: `frame`, the interval since the
  last frame (`Time<Real>`'s delta, the wall clock, after Bevy's time
  update: the virtual clock caps a delta at 250 ms); `main`, the main
  world's CPU time from the start of `First` to the end of `Last`; and
  `render`, the render world's `Render` schedule (natively pipelined beside
  the main world; it includes the wait for the display, so it reads about a
  frame at vsync). `debug_overlay::FrameTimes` holds them for the run
  recording too; `status::Status` keeps its own smoothed frame time and its
  debug log of slow frames (D1005), which the panel's fps line uses.
- **The spans.** The last 30 s of samples stay in a ring (always, so both
  windows are full when it is turned on; capped at 30 000 samples); the 5 s
  and 30 s rows (min, mean, p99, max of `frame`, mean and max of `main`) are
  computed from it at each redraw. The all-time row keeps running min, max
  and mean, a histogram of 0.25 ms bins to 250 ms for its p99 (">250" past
  it), and 200 buckets of min/max/mean that merge in pairs when full, so its
  memory is fixed however long it runs. Every frame counts but the first
  two (start-up), so an 800 ms frame shows in the max and as a white-capped
  column in the graphs.
- **The graphs.** Three 200 × 34 images (nearest sampling), one column per
  1/200 of the span: the mean as a solid orange bar, the spread up to the
  max dim above it, the main world's mean as a pale dot, guides at 16.7 and
  33.3 ms. The top is the first of 20, 34, 50, 100, 200, 500, 1000 ms at or
  over 1.25 × the span's p99, so one hitch does not flatten the rest; a
  column over the top is drawn to it with a white cap and counted in the
  label ("3 over").
- **The rest.** Mode and screen (`ui::mode_name`), level, car; in a race the
  tick, race time and state, speed, lap and gear; the backend and adapter,
  the window's size and scale; the entity count, pipelines waiting and late
  frames; on the web the wasm memory; the overlay's own cost.
- **Look and place.** Dark grey at 90 % opacity, orange text in Bevy's
  bundled Fira Mono (the `default_font` feature's bytes, registered once:
  the client's default font is Rajdhani, and Arimo's letters are not fixed
  width), 11 px. Bottom-left on the desktop, a corner the HUD leaves empty;
  with the touch controls (which take both bottom corners and the buttons
  under the place and time) centred under the progress bar. Scaled with the
  window from 1100 × 760 logical px down to 0.7 (text no smaller than
  8 px). `GlobalZIndex(i32::MAX)`: over the menus, the race, pause, results
  and the native loading screen; on the web the page's HTML loading screen
  covers the canvas until the scene is up.
- **Cost.** Text and graphs are redrawn four times a second, in place (the
  images' buffers reused, no allocation per frame but the text strings).
  Measured by itself in a release build on the RTX 3060, a race on Coast:
  0.005 ms a frame on average, 0.07 ms on a redraw frame (debug build 0.04
  and 0.5). Bevy's own share (relaying out a dozen lines of text and
  uploading three 27 KB images four times a second) is too small to see
  next to the machine's noise: two 45 s release races with and without the
  overlay gave main-world means of 6.6 and 10.0 ms, the difference the
  other agents' load (116 against 38 slow frames).

## D1021. The run recording: `record=1`, JSON lines

2026-10-05, the owner's request: "a more in depth game logging option so I
can record runs for you to examine". `recording`, the format in
`docs/rust-port/RECORDING.md` (DEVIATIONS).

- **On.** `record=1` (natively also `--record`) writes
  `recordings/<UTC date>-<time>Z-<level>.jsonl` under the repository root,
  where the agents work (git ignores it); `record=<dir>` or
  `record=<file.jsonl>` writes elsewhere. The path is printed at the start
  and, with the size, at the end. On the web the lines stay in memory for
  `__mp.recording()` and `__mp.saveRecording()` (a download); the native
  file is the priority, and a phone has no easy way to fetch a download,
  which is left open.
- **Format.** JSON lines, each with `t` (seconds since the start) and
  `type`: easy to grep by type, to read a line at a time, and to append to
  without a closing bracket, so a crash leaves a usable file (flushed every
  second). Written on change, not every frame: the status, mode and screen,
  settings, window, focus, pads, sound, overlay. Frame timing as one
  summary a second (count, fps, mean, min, p99, max of the interval; the
  main and render worlds' means and maxima; pipelines waiting) plus a
  `slow_frame` line for every frame over 50 ms (the D1005 threshold) with
  its number, the three times, the mode and pipelines waiting, so a stall
  is timestamped against what the client was doing. Natively the log's
  warnings and errors and the client's own (`mp_*`) info lines come
  through the tally layer (D1003), at most 1000 a frame; the web has no
  layer.
- **Races.** The start (seed, car, pursuit and heat, the units' cap,
  autodrive, timescale), pause, resume, results (with the table, lap times,
  pursuit or cruise stats) and the race going away. Every tick's input in
  `ticks` lines of 120 ticks (`Session::inputs`, added for it: the
  `InputFrame`s the session fed `race::step`, whichever device made them),
  run-length coded, about 12 bytes a tick; each line also carries the
  state's hash after its last tick, the player's car and the simulation's
  events (Debug text). A three-lap Seaside race at `timescale=3` (33 000
  ticks, 108 s) made 572 KB, 536 KB of it `ticks` (391 KB inputs, 70 KB
  events); racing costs about 1.5 KB a second, the rest a few hundred bytes
  a second.
- **Not recorded.** Raw key, pad and touch events (the inputs as the
  simulation saw them are, and they are what a replay needs), the camera,
  the effects, the sound's calls; per-frame timing (summarised per second;
  the overlay's graphs are for live looking).

## D1022. A recorded race replays in `mp-sim`

2026-10-05. The simulation is deterministic and a race is its options and
its inputs, so `mp-sim replay <file> [--race N] [--trace F] [--state-at T]`
steps each recorded race from a new `SimState` with the recorded inputs and
checks the hash at every `ticks` line (`mp_sim::replay`: the parser reads
only the flat fields of `race_start` and `ticks`, by hand, since the
simulation has no JSON dependency; the input coding is beside it, so the
client and the replay share it). It prints how many checkpoints matched (or
the first that did not, and fails) and the results; `--trace` writes the
trace of the race (`parity/trace-format.md`) and `--state-at` dumps the
state, which together give an agent everything the summary lines leave
out. The units' cap the client sets on a pursuit field
(`flow::apply_pursuit_opts`) is applied as there.

Checked: an autodrive race on Coast in a debug build (1802 ticks, 15
checkpoints) and a whole three-lap Seaside race at `timescale=3` (33 086
ticks, 270 checkpoints) replayed with every checkpoint matching and the
results the client showed; `recording::tests::a_recorded_race_replays`
drives a race by keys through the client's frame loop (ragged frames,
steering, handbrake, reset, a pause), writes its lines as the recorder
does, and replays them to the same hash at every checkpoint. A recording
replays on the code it was made with: the `start` line has the commit.

## D1009. Natively, finished one-shots are let go of

2026-10-06. The owner heard buffer underruns natively "well into level
play" (web-audio-api's `buffer underrun or overrun`, many a second).
Measured with the render thread's load report (`RUST_LOG=mp_game::play::
audio=debug`, D1005's companion in `play::audio`): on Coast with the
autopilot the load was 0.27 in the first ten seconds and 4 to 6 after two
and a half minutes, every device callback underrunning from then on. It
grew only while driving (parked: flat at 0.07), only with the effects on
(music alone: flat), and only with the exhaust pops (`shots::pop`'s noise
burst: a buffer source through a band-pass, a wave shaper and a gain into
the effects bus; without it, flat at 0.12). web-audio-api's own graph
report (its `diagnostics` feature) showed the render graph at 846 nodes
at the start and 4,790 two minutes in: finished pop chains, their handles
dropped and their source ended, still processing every quantum. A
reproduction against the crate alone (no facade) strands about one chain
in eight, and one plain source-gain-gain chain in thirty, so the cause is
in web-audio-api 1.7.0's freeing, not in the port; the exact path was not
found.

A browser collects such a chain once nothing references it. The native
backend now does the same for live contexts (`wa::native::Sweep`): a
processing node the facade lets go of is held instead of dropped; once
every node feeding it is gone (a released buffer source that has ended,
by its `onended`; a released oscillator past its stop time; or another
released node already let go of) for 4 s, past any filter's or reverb's
tail, it is disconnected and dropped, and the crate frees it. A node fed
by anything the facade still holds is never touched, and the links of
what is let go of leave `ins` (which before grew without end on the
effects bus). Offline contexts, which render the parity scenarios, keep
the crate's behaviour, so every audio golden is unchanged.

After: the same race holds 0.15 to 0.35 throughout (0.22 to 1.85 before at
the same machine load), underruns 1 to 2 % in 24 of 250 seconds (100 %
before). The `buffer underrun` messages the owner saw also turned the pops
into a rattle; the owner's report that the exhaust burble "sounds like
aluminum cans dragging behind the car" is to be judged again on this
build.

## D1060. A Quit button on the native menu

2026-10-05. The owner: "native game could use a quit button in the menu."
The native client's main menu ends with a QUIT button (`btn-exit`,
`Act::Exit`), a secondary `.btn` as the pause screen's: under the keyboard
or touch help, centred, and in the phone-landscape grid on a row of its own
across both columns (`touch=1` natively shows the touch layout). It leaves
the game as Ctrl+Q does (D1002): `AppExit::Success`, and `window_state`'s
`on_exit` saves the window on the way out. The web build has no such button
(the page cannot close itself), so the JS's menu and the web e2e suites
are unchanged. The gamepad reaches it with ↓ from the options row
(`ui::nav::nearest`, factored out of `mv_in` for a unit test); `uiscript=
btn-exit` presses it in a native run (checked: logs "Quit: exit", exit
code 0). `ActCtx` now carries the `AppExit` writer, which `ui_script` uses
too. Pictures: `parity/report/ev-audio/menu-quit-*.png` (native
`--screenshot` at 1280 × 1500 and, with `touch=1`, 1000 × 480).

## D1061. The Ion Arc's motor is hushed and its wind is louder

2026-10-05. The owner: "the ion arc engine sound is really annoying at full
speed. I have seen other games make EVs silent and play up car wind noise."
`update_electric` scales the motor's tonal voices (f1, f2, f3, the gear
mesh, the regen whine and the filtered saw) by `ev_hush(motor)`: 0.45
(−7 dB) up to 10 % of the motor's top speed, falling linearly to 0.125
(−18 dB) at 60 % and above. The idle hum at a standstill, the boost growl
and the rotor-air noise are as before. `update_environment` multiplies the
wind bed and the high wind beds by `ev_wind(speed)` for the electric car:
1 at a standstill rising to 2.25 (+7 dB) at 60 m/s and beyond, and the road
rumble by its square root (half as many dB). Every other car multiplies by
1 (exactly), so their sound and the call logs are the JS's.

Measured offline (`render_scenarios` on the native backend, SFX 0.85, RMS
over 0.5–1.5 s of a steady state at full throttle, power 450 kW, motor =
speed / 71): the motor alone (speed 0) goes from −15.9 / −16.2 / −16.3 /
−16.3 / −16.3 dB at 5 / 15 / 30 / 50 / 71 m/s to −22.7 / −24.5 / −28.5 /
−34.2 / −34.2 (−6.8 to −17.9 dB); the wind and road alone go from −53.2 /
−43.0 / −36.1 / −30.3 / −27.4 to −52.7 / −41.8 / −33.6 / −26.1 / −21.5 (up
to +5.9 dB). At top speed the motor was 11 dB over the wind and is now
12.7 dB under it. A 12 s run from 0 to the limiter: −16.4 dB overall before
(the motor's, constant), −24.1 after, rising with speed from −27.6 (28–43
m/s) to −21.5 at the limiter. The sports car's renders are bit-identical
before and after. Renders to listen to: `parity/report/ev-audio/`.

The audio golden is Chrome's render of the JS, so the ten
`engine-electric-*` scenarios now differ from it on purpose (−6.9 to −18.1
dB RMS). `tools/parity/audio-bands.mjs` holds them (its `DEVIATED` list) to
the Rust render's own bands in `parity/golden/audio/rust-deviated.json`,
written with `--update-deviated`, at the same 1.5 dB; the other 101 are
still compared with Chrome (110 of 111 pass; the one left is D259's
`engine-rally-6500-1` at 25 Hz, as before). The JS game is unchanged: the
same change in `src/game/Audio.js` (`_updateElectric`,
`_updateEnvironment`) would bring it to GitHub Pages if the owner wants.

## D1062. The gamepad's camera button is Y, as it was

2026-10-05. The owner was not sure the pad had the camera view and asked
for a face button or RB. It has: the JS's `DEFAULT_MAP` binds `camera` to
Y (button 3; gilrs's North natively), the one-shot that C is on the
keyboard, and the Controller screen lists it (Camera: Y) and remaps it
like the others. RB (button 5) is the handbrake's second button (X / RB),
so moving the camera there would take it from the handbrake. No binding
changed, so saved maps (`mr.padMaps`) load as before. A unit test pins it
(`the_camera_is_y_by_default_and_rb_stays_the_handbrakes`: the default
map, the label, one camera press per Y press, RB the handbrake's, no
button bound twice, a saved map keeping its own camera button).

## D1040. The bumper view does not draw the player's own car

2026-10-05, the owner: "the in car views ... are occluded by the car".
`CameraRig.js`'s one in-car mode, `bumper`, puts the eye 1.2 m ahead of
the car's origin and 0.72 m up (looking back, 2.3 m behind and 1.02 m
up), which is inside the body, under the bonnet; the JS draws the same
thing (pictured with `?parity=1` and the rig set to bumper: the lower half
of the screen black). Ray-cast in the JS page, the black is the GT's front
wheel-arch liner (the body's `trim` bucket, radius 0.41 m round the axle
at z = 1.3): the eye is inside it and its inward faces, which three's
FrontSide culling keeps, fill the view below the horizon. Other kinds
show other insides. Nothing in the JS hides the player's model, sets a
layer or moves the near plane (0.3 m) for this mode.

Fixed in the port, the camera itself unchanged: while the drawn eye (the
rig's, shake and intro included) is inside the player's car, its box from
the model's own vertices grown by the near distance (`play::incar`), the
car's mesh entities are hidden. The root, the headlight anchor (the spot
reads its transform, not its visibility) and the flames hung on the body
stay, so the spot, the pools and the flames are as before; like three's
`visible = false`, the hidden meshes cast no shadow then. The chase, far
and intro views never come within the box (tested), so only the bumper
view changes. A bug the owner reported, so the JS should take the same
fix (CLAUDE.md, the precedent of D807); not done here: a change under
`src/` changes the parity cache key. The JS change: in `Race.js`, after
the player's model is built, keep its meshes (`pModel.root.traverse`
already visits them) and its box (`new THREE.Box3().setFromObject(
pModel.root)`, the root still at the origin), and after the camera is
placed each frame (after `introCamera`) set every mesh's `visible` to
whether `camera.position`, brought into the root's frame
(`root.updateMatrixWorld(); root.worldToLocal(p)`), is outside the box
grown by 0.3. Until then DEVIATIONS.md lists it.

Tests (`play::incar`): `bumper_view_sees_the_road` builds each car kind's
model as the race does, places the bumper eye ahead and looking back on
sierra, coast and streets at three moments of an autopiloted race, and
casts 25 rays through the lower middle of the view against the model's
triangles as drawn (culled as three culls): none meets a drawn triangle,
and the middle ones come down on the road within its width. It also
checks its teeth: drawn whole, the GT stops most of the forward rays.
`chase_views_keep_the_car`: the intro, chase and far eyes are never
inside the box.

## D1041. `?camera=chase|far|bumper`: a race starts in that mode

2026-10-05. A test hook for pictures of the in-car view (D1040): the
race's camera rig starts in the named mode, as if C had been pressed
during the countdown (`Params::camera`). The JS has no such parameter;
its rig is `window.__race.cam` (`cam.mode = 2; cam.snap = true`).

## D1042. The headlight pools lie on the road, not over the tyres

2026-10-05, the owner: "when we pan around the car at game start I can
see that the tires are drawn as partially embedded in the road". Not the
wheels: each tyre's lowest point sits on the road at the start (in the
JS, 1.9 mm above it on coast, the 30-sided tread's flat; in the port,
within millimetres on every level and car kind; and the JS's road mesh is
where `surfaceY` puts it, ray-cast across the road). The JS draws the same picture,
and hiding the pools takes the sunk look away (JS pictures with and
without them). `Effects.js`'s headlight pool is a level quad at its car's
`y + 0.06`, centred 7.5 m ahead (10 for the player), 7 × 12 m (9 × 16),
additive, with `polygonOffset(-6, -6)`. On the grid each pool of the row
behind lies under the row ahead's cars; the quad 6 cm up, pulled further
toward the camera by the offset's slope term, draws over the bottom of
their tyres, which then read as road. The same happens wherever a car
drives over another's pool, at night.

Fixed in the port (`Effects::lay_pools`, after `update`, in the race and
both staged scenes): each visible pool sits `POOL_LIFT` (1 cm) over the
road at its centre (`surfaceY` at its projection), tilted with the road
there (grade along, bank across, as `Vehicle.sync` tilts a car), its
long axis still along its car's heading; its material keeps the constant
offset but drops the slope term (factor 0). A pool now covers at most a
centimetre of a tyre, under a pixel at the start's distance. Tilted, it
also follows grades the level quad at the car's height fell under (D803's
reason for the slope term). In a dip the far ends of a 16 m quad can
still sink under the road (sagitta L²/2R); the glow fades there anyway.

Test: `tyres_sit_on_the_road_at_the_start` (`play::incar`) checks on
every level and car kind that the player's tyres' lowest points sit
between 5 mm under and 3 cm over the road, and that no racer's pool lies more than its lift
over them, after asserting that as the JS lays them, pools cover the
bottom 3 cm or more of the tyres.

The JS change (the owner's rule for reported bugs, D807; not made here):
in `Effects.update`, with the track passed to `Effects` (Race.js has it),
after placing a pool: `const P = track.project(x, z, v.s); const f =
track.frame(P.s)`; `pool.position.y = track.surfaceY(P.s, P.lat) + 0.01`;
the quaternion from the basis (x = y × z, y = (A × T) normalised with T =
(f.fx, f.grade, f.fz), A = (f.rx, −f.bank, f.rz), z = the heading
(sin, 0, cos of the old `rotation.y`) made square to y), as
`Vehicle.sync` builds the car's; and `polygonOffsetFactor: 0` on
`poolMat` (`tailPoolMat` is never drawn). Until then DEVIATIONS.md lists it; the
staged effects scenes (D804) differ from the JS's by it.

## Driving aids for phones (Rust only): the guide line, the steering assist, bigger touch controls

The owner, 2026-10-05: "the game is really fun with a gamepad, but it's
pretty rough on mobile. In vertical mode the controls are just too small,
and it is still hard with the phone in landscape. can we add options, on
by default on mobile, for steering assist and on-road indicators for the
best line, which change from green to orange to red based on your current
speed to signal when to slow down?" The JS game has none of this; each is
a row in DEVIATIONS.md.

## D1080. The guide line's colour: how soon you have to brake

2026-10-05. The line is the track's `racing_line` (the lateral offset per
metre the rivals follow), and each point on it has a target speed, the
track's `speed_profile`: the speed a well-driven car carries through
there, already limited by braking for what follows (`build_racing_line`:
`sqrt(15.5 / |curvature|)`, capped at 69.5 m/s, then a pass from the end
backwards holding each metre to `sqrt(v_next² + 2 × 11)`, twice round a
loop). So a point's target speed already says "slow down here for the
corner after it"; the colour only has to compare it with the player's
speed and the room left.

What the car can do comes from `mp_sim`'s physics, read off its code:
full brake is `a -= 15 × brake` for every car (above 0.8 m/s), plus the
drag `0.00115 v|v| + 0.01 v`; `mp_sim::assist::BRAKE_DECEL` and
`full_brake_decel` hold these, and a test steps the physics one tick at
full brake and checks the deceleration (less the slope) against them.
`brake_distance(v0, v1, frac)` is the distance to slow from v0 to v1 with
`frac` of the brake and the whole drag, ∫ u / a(u) du by Simpson's rule
over eight steps (flat road; the grade is left out).

The rule (`mp_sim::assist::brake_urgency(v, vt, d)`, for a point d metres
ahead along the road with target speed vt, the car at v):

- at or under the point's speed (v ≤ vt), or nearly still (v < 1 m/s):
  green;
- otherwise D = `brake_distance(v, vt, 0.75)` is the distance needed
  braking firmly (three quarters of the pedal, about the profile's own
  11 m/s², with a quarter in hand), and t = (d − D) / v is the time left
  before the car must start braking for that point;
- t ≥ 1.5 s: green (comfortably reachable); from 1.5 s to 0.75 s it turns
  orange; orange to 0.25 s ("brake soon"); from 0.25 s to 0 it turns red;
  t ≤ 0: red ("brake now"; further past it the point cannot be made even
  on full brakes, which is still red: there is nothing redder).

As a number: urgency u = clamp((1.5 − t) / 0.75, 0, 1) + clamp((0.25 −
t) / 0.25, 0, 1), 0 green, 1 orange, 2 red, with the colours between
interpolated in linear RGB (green #28E15F, orange #FF9614, red #FF2323).
Unit tests pin the bands, monotonicity in distance and speed, and green at
or under the target. Each chevron is coloured by its own point, so
approaching a corner too fast the line turns red from the corner back
towards the car; once the car is slow enough the far side of the corner,
where the profile rises, stays green.

Braking only shows a chevron with its urgency as its opacity up to 1 (so
nothing while green, fading in as it turns orange); Full shows every
chevron.

## D1081. Drawing the guide line

2026-10-05. `play::guide`: one mesh of 102 chevrons, six vertices and four
triangles each (a ">" pointing down the road, 1.5 m across, the tip
0.9 m ahead of the arms' ends, the arms 1.5 m long along the road),
with positions, an up normal and RGBA vertex colours; an unlit (`Model::Basic`)
transparent material, both sides drawn, no depth write, fog on, and the
skid marks' polygon offset (constant 256, slope 4, as `skid_material`); 5 cm over the
road surface (`Track::point_at`'s height, which includes the bank; the
skid marks sit 3 cm up). The chevrons sit every 3 m of arc length at
fixed places on the road, so they do not crawl as the car moves; each
frame the ones within reach are placed along the racing line
(interpolated between samples) and bent with the road, and the others
collapse to nothing. Reach: 5.5 s of road at the car's speed, 140 to
300 m; opacity 0.9, faded in from 3 to 9 m ahead (under the car and the
bumper camera) and out over the last 45 % of the reach. The first try had
arms 0.32 m thick: from the chase camera, 1.5 m up, a flat shape on the
road shows about a tenth of its length 15 m away, and they drew as green
hairlines; the arms are now as long as the chevron is wide, with 1.5 m
gaps, which reads as a row of arrows.

The vertices go into the mesh's place in the vertex slab once a frame
(`animate::MeshWrites`, as the smoke and sparks, D701); no asset edit, no
new mesh. The CPU copy is reused; the write itself packs a vertex buffer
(24 KB), as the particles' writes do. The entity sits at the middle of
the line's reach and the vertices are written relative to it, so the
transparent sort uses that point (D808's way); its sort rank is 6, after
the effects, so where it ties with the skid marks it draws over them. Its
pipeline joins the race's warm-up (D458). Hidden when Off, on the
results, after the player's finish, and before the race has started
drawing. The speed and place are the player's as drawn (between the last
two ticks).

`__mp.aids` (web) gives `guide`, `assist`, `shown` (chevrons drawn last
frame) and `maxUrgency` for the tests.

## D1082. The steering assist

2026-10-05. `mp_sim::assist::steer_assist(inp, vehicle, phys, track,
Assist)`, a pure function through the kernel. The client calls it on the
player's controls each tick before quantising them into the tick's
`InputFrame` (`flow::Race::frame`), so the frame (and so any recording or
trace) carries the assisted steering and replays exactly; the simulation
itself never calls it, so the race, its traces and every parity test are
untouched, and with the assist Off it is not called (and is the identity
if it is: a test). The autopilot never gets it.

- **Where the line wants to go**: the turn that arcs the car from its
  place and heading onto the racing line `8 + 0.5 × speed` metres ahead
  (the circle tangent to the heading through that point: curvature
  2 sin α / L), as a yaw rate, then the wheel angle physics would need for
  it (`atan(yawRate × wheelBase / speed)`), as a steering input for these
  controls: divided by the lock physics allows at this speed for this
  kind of input (`steerMax`, including the analogue scaling to the grip),
  clamped to ±1.
- **How much help is needed**, 0..1: heading off the road (the car's
  edge, at the place its drift across the road takes it in 0.7 s, within
  2 m of the road's edge rises to 1 at 0.2 m; the present place counts with
  0.8 m less), or far off the line (4 to 8 m off, counted half; the first
  try started at 2 m, which nudged a car going straight down the middle
  of a two-lane road, and so changed the field it raced in).
- **Blend**: steer ← steer + (line − steer) × strength × need × (1 − 0.8 ×
  deliberate), where deliberate = smoothstep(0.5, 0.95, |steer|): a hard
  steer of the player's own keeps at least four fifths of its way.
  Strength: Light 0.5, Strong 0.85.
- **Never**: slower than 4 m/s, in the air, in the countdown, or facing
  more than 1.2 rad off the road (spun, or the wrong way). No braking or
  throttle: only the steering changes.

Measured headlessly (`cargo test --release -p mp_sim --test assist --
--ignored --nocapture`): a scripted driver uses the autopilot's pedals
(93 % of the profile, no nitro) with the stick flagged analogue, and
steers as one of: Good (the autopilot's steering), Lazy (half of it),
Noisy (it plus a random ±0.7 held for a quarter second), Pulling (70 % of
it plus 0.2 to the right); a reset after 3 s stuck. Off-road is the car's
centre outside the road's half-width while racing; Sports car, seed 1,
the whole race:

| Level | Driver | Race time off / Light / Strong (s) | Off-road off / Light / Strong (s) | Resets off / Light / Strong |
|---|---|---|---|---|
| Sierra | Good | 208.6 / 198.4 / 209.4 | 5.1 / 0.6 / 1.2 | 0 / 0 / 0 |
| Sierra | Lazy | 233.8 / 227.5 / 260.2 | 29.6 / 10.4 / 2.2 | 0 / 0 / 9 |
| Sierra | Noisy | 227.3 / 226.5 / 203.7 | 17.7 / 3.6 / 2.5 | 0 / 2 / 0 |
| Sierra | Pulling | 217.7 / 217.1 / 210.1 | 21.0 / 5.8 / 1.0 | 0 / 1 / 1 |
| Coast | Good | 150.4 / 157.8 / 148.6 | 4.0 / 0.0 / 0.0 | 0 / 0 / 0 |
| Coast | Lazy | 165.8 / 147.7 / 161.7 | 15.4 / 4.3 / 0.8 | 0 / 0 / 1 |
| Coast | Noisy | 151.7 / 146.0 / 146.3 | 7.9 / 2.9 / 0.4 | 0 / 0 / 0 |
| Coast | Pulling | 163.8 / 156.6 / 169.5 | 11.4 / 3.2 / 0.1 | 0 / 0 / 1 |
| Desert | Good | 136.9 / 140.1 / 147.5 | 2.7 / 0.5 / 4.1 | 0 / 0 / 1 |
| Desert | Lazy | 162.4 / 139.0 / 142.7 | 19.6 / 5.8 / 1.8 | 0 / 0 / 0 |
| Desert | Noisy | 139.7 / 144.5 / 144.6 | 12.4 / 3.8 / 2.7 | 0 / 0 / 0 |
| Desert | Pulling | 172.0 / 173.3 / 152.9 | 16.9 / 2.5 / 0.7 | 5 / 4 / 0 |
| Streets | Good | 164.3 / 156.4 / 154.0 | 0.0 / 0.0 / 0.0 | 1 / 0 / 0 |
| Streets | Lazy | 192.9 / 168.0 / 165.1 | 0.0 / 0.0 / 0.0 | 4 / 0 / 0 |
| Streets | Noisy | 163.1 / 171.0 / 159.7 | 0.0 / 0.0 / 0.0 | 1 / 2 / 0 |
| Streets | Pulling | 161.8 / 163.0 / 169.2 | 0.0 / 0.0 / 0.0 | 0 / 0 / 0 |

Off-road time falls by 63 to 85 % with Light and by 78 to 99 % with
Strong for every poor driver on the three levels with open road edges.
Race times move by a few seconds either way: the field's contacts differ
from run to run, and most resets are the car pinned against another one
(Sierra's Lazy and Strong run: nine resets behind a car stopped at 4.7 km,
which the scripted driver never steers round, hence its 260 s). A good
driver's times move as the others' do (Light: Sierra −10 s, Coast +7 s);
with Strong, Desert's good run spent 1.4 s more off the road than
without (not looked into further), one reason for Light as the default. Streets has walls at the road's edge, so nobody
leaves it; there the assist saves the bounces (Lazy 193 → 168 → 165 s).

Unit tests: Off is the identity; heading for either edge with the stick
at rest it steers back, Strong more than Light; on the line it leaves the
steering; full lock toward the edge keeps most of its way; nothing when
spun, crawling or airborne.

## D1083. The settings, the defaults and the hooks

2026-10-05. Two drop-downs join the menu's options, on every device:
**Guide line** (Full, Braking only, Off; `mr.guideLine` = `"full"`,
`"brake"`, `"off"`) and **Steer assist** (Off, Light, Strong;
`mr.steerAssist` = `"off"`, `"light"`, `"strong"`), saved as the other
settings are (JSON strings under `mr.`), ids `opt-guide` and
`opt-assist`. Defaults: on a touch screen (the same decision as the touch
controls, `ui::touch_ui`) Full and Light; with a keyboard or gamepad, Off
and Off. Light rather than Strong so the player still steers (the owner
asked for help, not a driver). A saved value the menu does not offer
falls back to the default. `__mp.settings` gains `guide` and `assist`;
`__mp.selects` the two lists.

Query hooks (D1004's pattern, natively through `--query`): `line=full|
brake|off` (also `1`, `0`) and `assist=0|1|2` (also `off|light|strong`),
which override the setting for the run. `play::guide::settings` hands the
setting (or the hook) to the race each frame before its ticks, so a
change in the menu reaches the next race and a running one.

## D1084. Bigger touch controls

2026-10-05. Looked at on the emulated iPhone (390 × 844, both ways up):
the JS's sizes (D436, D840) are the same in CSS px both ways up, sized by
the short side, so upright, with twice the height, they look small and
use little of the screen, and the stick's full lock is 58 px of thumb.
Rust only:

- Sideways: `--b` 14vmin + 26 px (60 to 104; was 12vmin + 26, 58 to 96):
  72.8 → 80.6 px on the iPhone; the slider 66vmin (200 to 320; was 62vmin):
  242 → 257 px tall, as wide as the JS's (D1085); full lock 0.17 of the
  short side (was 0.15): 58.5 → 66 px.
- Upright: `--b` a fifth of the width, held to (w − 62) / 4.16 so the
  Buttons choices' ◂ ▸ and pedal pads still share the bottom row (78 px);
  the ◂ ▸ pads and the GAS and BRAKE pads 1.4 times as tall (118 and 142
  px), DRIFT and N₂O 0.84 b round (was 0.74 b); the slider panel's unit
  0.22 of the width (86 px: slider 81.5, strip 60, was 69 and 51) and the
  slider 0.36 of the height (304 px, was 242); full lock 0.185 of the
  width (72 px), the most that keeps the stick at rest clear of the panel.
- The knob is the full-lock distance across, 56 to 72 px (56 in the JS),
  and its track is 8 px shorter than the knob (48 in the JS).
- The HUD's `--steer-top` for the Buttons choice (what keeps the speed
  box above the ◂ ▸ pads) is now the pads' own top, so the taller pads
  push it up; sideways it is the same number as before.

The suites against the release build (`js-suites.mjs`): touch-controls
10 of 10, tilt 5 of 5, race-flow 11 of 11, menu 9 of 9 (the new options
included), analog-controls 4 of 5. The one that fails, "the pedal
slider ...", fails on its last check ("light gas pulls away slower"): it
resets the car 40 m past the start at rest at about 4 s of race time
(`timescale=2`), in the path of the field, and a rival running into it
gives the "light gas" pull 12 to 15 m/s. Replaying the test's steps by
script, main's build is hit the same way; whether the real test is hit
depends on how much race time its wall-clock steps take, which this
build shifts (it fails here also with the guide line and the assist
off, once in two). Left for the owner's call: the test is the frozen JS
game's.

Pictures (rest, both thumbs, the Buttons choices; before = main's build,
after = this one), the guide line on Sierra (day) and Streets (night)
into a hairpin flat out, sideways and upright, every camera mode, Braking
only, the menu's new options, and the native countdown with the line (lavapipe under Xvfb: the
race shot 20 s in did not come within 15 minutes on the loaded machine):
`parity/report/driving-aids/index.html` of the branch's worktree (build
output, not in git; `tools/parity/e2e/guide-line.mjs` and
`tools/parity/touch-shots.mjs --side rust` make them again).

## D1010. The menu's music plays at once

2026-10-06. The owner, natively: no game sound on the menu until a volume
slider was moved (that applies the volumes and turns the music on). The
JS menu is the same: `wakeAudio` builds the sound on the first gesture,
but nothing picks a track until a level tab (`if (audio.ready)
pickMusic()`) or Race (`startRace`: `pickMusic(); applyVolume()`). The
owner wants the music at once. On the main menu, once the sound is up,
`play::audio::RaceAudio::menu_music` picks the selected level's track and
applies the volumes, as `startRace` does; natively, where nothing needs a
gesture, it also wakes the sound itself, so the music starts with the
menu. On the web the sound still waits for the first tap or key (the
browser's autoplay rule). A deviation (DEVIATIONS.md).

## D1085. The sideways slider keeps the JS's lane: headless Chrome's 800 px touch region

2026-10-06. After D1084, analog-controls' "the pedal slider ..." test
failed against the Rust build every time, with the driving aids off
(`light gas pulls away slower (15.3 vs 11.0 m/s)`), and passed on main
before it. Not the simulation: logged step by step, both builds put the
field in the same places at the same race time. The test's steps took
more race time: each slide (a CDP `touchMove`, the harness's two frames,
an 80 ms sleep) took 10 frames instead of 8 or 9, so at `timescale=2` the
test reset the car at s = startS + 40 about 0.25 s of race time later,
just as the five rivals came through there, and one rammed it during the
"light gas" pull.

Why a slide took a frame longer: on the emulated 915 × 412 phone
(puppeteer's device emulation of a browser whose window was 800 px wide),
Chrome dispatches DOM touch events only for touches left of x = 800 CSS
px, a sharp edge at every height, whatever `--window-size` or
`defaultViewport` say. Left of it, a touchMove goes to the page as a
`touchmove` (behind winit's non-passive `touchstart` listener on the
canvas, aligned to the next frame) and CDP acks it after about 45 ms;
right of it only pointer events go, acked after about 33 ms. The game
reads pointer events, so it sees the same input either way; only the
test's clock shifts. D1084's wider sideways slider moved the slider's
centre on that device from x = 804 to 795, across the edge.

So sideways the slider keeps the JS's width and lane (its unit is the
JS's `--b`, clamp(58, 12vmin + 26, 96)) and grows only in height; the
pads, the stick and the upright layout keep D1084's sizes. The slide
steps are 8 frames again and the suite passes. The edge is a property of
the emulation, not of phones; a test whose outcome hangs on where the
field is when it drops the car in its path stays fragile, and putting
the car clear of the field (say 400 m on) would end that, but the test is
the frozen JS game's.

## D1086. The owner on the driving aids

2026-10-06. After playing on the iPhone: "mobile feels good with the driving
line and assist!" The touch defaults stay as D1083 set them: guide line
Full, steering assist Light.

## D1100. The Rust game is Midnight Playground; its crates are `mp_`

2026-10-06. The owner: "i want to rename this - midnight playground. and
rename the mr_ rust variables/crates". This answers SPEC open question 6
and vision WORLD 1.2. The owner's choices, asked: prefix `mp_`; the code,
docs and the Rust game's own name now, the public addresses at cutover;
the JS game keeps its name.

Renamed now:

- The eleven crates `mr_*` → `mp_*` (directories, package names, every
  path), the `mr_webgl2` cfg → `mp_webgl2`, the binaries `mr-sim`,
  `mr-host` → `mp-sim`, `mp-host`, the client's binary `midnight-racer` →
  `midnight-playground`, and the wasm files `mr_game_bg.wasm` →
  `mp_game_bg.wasm` (and the WebGL2 build's).
- The page bridge `window.__mr` (and `__mrShim`, `__mrFonts`,
  `__mrSceneExport`, `__mrFacade`) → `__mp`; the test and tool
  environment variables `MR_*` → `MP_*` (`MP_TARGET`, `MP_BASE_URL`,
  `MP_HEADFUL`, `MP_STORE`, `MP_MUTE`, …); GPU labels and shader locals.
- The name the player sees in the Rust build: the window title
  (`Midnight Playground`) and Wayland app id / X11 class
  (`midnight-playground`), the page's title, the loading screen's and the
  menu's logo (MIDNIGHT over PLAYGROUND, with RACER's style; the
  per-letter gradient spans the same stretch over ten letters), the
  banner, the recording's file name. A deviation (DEVIATIONS.md).
- The native settings folder: `midnight-playground/` in the config
  directory; the first time it is missing, the files of `midnight-racer/`
  (storage and window state) are copied into it.

Kept, and why:

- **Everything of the JS game** (`src/`, `index.html`, `vendor/`, its
  manifest, music page, Electron shell, README until WP 9.4): it is frozen,
  becomes `/legacy/` at cutover under its own name, and `src/` is the
  parity cache key. Its comments still name `mr_math`.
- **`mr_kernel`** (the crate and `tools/parity/kernel/mr_kernel.wasm`):
  `src/parity/kernel.js` loads that file by name and its bytes are part
  of the JS tree key. The committed wasm is unchanged; `cargo xtask kernel
  --check` passes on it.
- **The shader defines `MR_PHOTO` and `MR_SUN_FOG`**: the JS's names,
  ported as they are.
- **Storage keys `mr.*`** (web localStorage and the native file): the JS
  game writes the same keys on the same origin, which is how its best
  times carry over at cutover (WP 9.5).
- **File formats**: the magics `MRSCENE`, `MRTRACE`, `MRSURVEY` and the
  `.mrscene` extension. Golden traces in git carry `MRTRACE`; a format tag
  is not a name the player sees.
- **In-world signs** ("MIDNIGHT RACER" on the raceway, "STAGE 1 ·
  MIDNIGHT RACER"): level content ported from the JS; vision WORLD 1.2
  keeps *Midnight Racer* as the street-racing series.
- **Addresses until cutover**: the GitHub repository and Pages address,
  the local folder, the `proj` registry name and the tailnet path
  `/midnight-racer/`, and the tests' stand-in origin
  `https://midnight-racer.test`. A repository rename moves the Pages
  address, and GitHub does not redirect a project site, so it is done once,
  with the swap (WP 9.5).

## D1101. Pages publishes the Rust build beside the JS game

2026-10-06. The owner, so friends can try the Rust build while
multiplayer is built: "yes, side by side deploy", having been offered
this or a full cutover and noting they "never did comprehensive testing"
of the Rust version. This brings forward part of WP 9.5 (the Pages
source moves to GitHub Actions) without the swap.

- `.github/workflows/pages.yml` runs on every push to `main` (cloud
  workers push there too) and publishes the commit's tracked files, so
  the site root is what "deploy from branch main" served, plus
  `cargo xtask web --release` at `dist/next/`: the same path as on the
  registered server and the tailnet, so the client's relative URLs
  (`../../audio/radio/`, `../../music.html`) resolve as they do there.
  The `.br` and `.gz` copies are left out, since Pages compresses on the
  fly.
- A failed Rust build still publishes the JS game. `dist/next/` is then
  a one-line page naming the commit that failed. Deploys queue one at a
  time, and only the newest waiting push is kept.
- The JS game stays at the root, frozen as before. Both games share the
  origin, and so the `mr.*` storage keys, as they always have on the
  tailnet (D1100).
- Still the owner's to decide at M9: the swap itself, `/legacy/`, and the
  repository rename.
