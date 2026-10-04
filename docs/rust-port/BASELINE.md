# Performance baselines of the JS game

What the Rust build is measured against (SPEC 6.6, roadmap WP 0.8 and gate
G1). The JS game is measured on the same devices, the same way, before the
Rust build exists.

## How to read the numbers

Open the game with `?stats=1`. A panel in the bottom-left corner shows,
refreshed twice a second:

```
fps 58.9  cpu 4.12ms  worst 21.3ms
calls 702  tris 2104k
load 4.31s  heap 188MB
```

- **fps**: frames in the last half second. **cpu**: the game's own time per
  frame (JavaScript, not the GPU). **worst**: the longest frame in that half
  second (a hitch shows here).
- **calls**, **tris**: draw calls and triangles of the last frame, shadow
  and bloom passes included.
- **load**: seconds from opening the page to the menu (or to the first
  frame of the fly camera). **heap**: JavaScript memory, shown only by
  Chrome; Safari does not report it, so on the iPhone it is absent.

## Phones (the owner reads these)

The reference phones are named by the owner and joined to the tailnet
(ROADMAP M0). Use the tailnet https address of the dev server, which serves
the working tree: `https://loaf.cama-minor.ts.net/midnight-racer/`. Settings
as a player has them on a phone (high quality off, the default on touch
devices). For each row, open the link, wait for the panel's numbers to
settle, and note them; for the fly-camera rows, note the lowest fps and the
largest worst frame seen while it flies, and how long the flight lasts
before it reaches the end.

| What | Link (append to the address above) |
|---|---|
| Load to menu, Sierra | `?stats=1&level=sierra` (read `load` once the menu is up; reload twice and take the middle value) |
| Fly camera, Sierra, 60 m/s | `?stats=1&level=sierra&s=80&v=60` |
| Fly camera, Coast, 60 m/s | `?stats=1&level=coast&s=80&v=60` |
| Fly camera, Streets | `?stats=1&level=streets&s=80&v=60` |
| Fly camera, Desert | `?stats=1&level=desert&s=80&v=60` |
| Fly camera, Seaside | `?stats=1&level=seaside&s=80&v=60` |
| Fly camera, Night City Cruise | `?stats=1&level=cruise&s=80&v=60` |
| A race, Sierra, autopilot | `?stats=1&level=sierra&autostart=sports&autodrive=1` |

Record the results here:

| Device | OS, browser | Level | Load to menu | fps (typical / lowest) | Worst frame | Calls | Tris | Notes |
|---|---|---|---:|---:|---:|---:|---:|---|
| iPhone (model?) | iOS 26.?, Safari | sierra | | | | | | |
| Android (model?) | Android ?, Chrome | sierra | | | | | | |

G1 flies the Rust build at the same speed (60 m/s) along the Sierra and
Coast exports and compares its frame time with these rows on the same
phone.

## Desktop

Measured by `node tools/parity/perf-baseline.mjs` (headless Chrome on the
dev machine's RTX 3060, frame rate uncapped, high quality, fly camera at
60 m/s from s = 80 m to the end of the route or four minutes; load is with
the files served from disk by the test harness, so it is not a network
figure). The race figures with the autopilot are in SPEC 6.6.

| Level | Flown | Load to menu | fps median | fps 5th pct | Worst frame | CPU per frame | Peak calls | Peak tris | Peak heap |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| sierra | 170 s | 3.09 s | 507.4 | 229.8 | 209 ms | 1.94 ms | 502 | 3.31 M | 177 MB |
| coast | 138 s | 2.75 s | 458.6 | 299.6 | 327.4 ms | 2.09 ms | 449 | 3.29 M | 334 MB |
| streets | 88 s | 3.24 s | 591.6 | 288.8 | 166.5 ms | 1.49 ms | 362 | 1.77 M | 483 MB |
| desert | 129 s | 3.92 s | 393.1 | 221.5 | 127.1 ms | 2.32 ms | 390 | 3.03 M | 114 MB |
| seaside | 60 s | 1.39 s | 751.5 | 523.8 | 106.7 ms | 1.12 ms | 180 | 1.25 M | 63 MB |
| cruise | 239 s | 3.22 s | 617.4 | 325.5 | 42.4 ms | 1.59 ms | 292 | 2.19 M | 346 MB |

Measured 2026-10-03 with the machine busy with unrelated work (load average
about 20 on 24 cores), so the CPU figures and the worst frames are high;
the GPU-bound frame rates are the useful part. Rerun on a quiet machine
before comparing frame times closely. "Worst frame" includes the first
frames after the page appears, when shaders compile.

## Simulation speed (WP 1.7)

SPEC 4.6 test 6: ticks per second for the full Sierra field (the player
on the autopilot, five rivals, 22 traffic cars, collisions; no Race rules,
no trace), median of five runs of 14,400 ticks, on the dev machine (Ryzen 9
9900X). The Rust figure is `cargo run --release -p mr_sim --bin mr-sim --
bench`; the JS one is `parity/golden/sim/bench.json`
(`tools/parity/sim-bench.mjs`, Node, parity kernel on).

| | ticks/s | best | load average |
|---|---:|---:|---:|
| JS (Node 22, kernel on) | 86,766 | 89,271 | 16.4 |
| Rust (native, release) | 143,545 | 244,791 | 26.5 |

Both measured 2026-10-03 on the shared machine; the spread between runs
is the load. A whole race with the Race rules runs at 130,000 to 180,000
ticks/s natively (about 1,100 to 1,500 times real time).

## The Rust client at WP 2.1 and 2.2 (first measurements)

The Bevy client (stand-in materials, DECISIONS D103) loading each export,
on the dev machine (RTX 3060), 2026-10-03, with the machine busy with
unrelated work (load average 20 to 35 on 24 cores), so these are
indications, not baselines. Native: the dev build (`cargo run -p mr_game`,
dependencies optimised), `--screenshot`, from start to the screenshot of the
first frame with every pipeline compiled; web: the dev wasm in headless
Chrome on WebGPU (Vulkan), through the registered server, times from
navigation.

| Level | Export | Entities | Materials | Native: start to ready | Native peak RSS | Web: first frame | Web: download | Web: parse | Web: ready |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| sierra | 138 MB | 63,670 | 119 | 4.1 s | 839 MB | 1.4 s | 0.6–0.8 s | 0.11–0.22 s | 3.3–5.7 s |
| coast | 210 MB | 28,673 | 143 | 3.6 s | 929 MB | 1.2–1.4 s | 0.6–1.0 s | 0.16–0.29 s | 3.7–4.7 s |
| streets | 166 MB | 15,217 | 44 | 2.6 s | 861 MB | 1.2–1.3 s | 0.6–0.7 s | 0.22–0.28 s | 4.0–5.7 s |
| desert | 107 MB | 19,117 | 62 | 2.5 s | 714 MB | 1.4 s | 0.1–0.5 s | 0.18 s | 4.4 s |
| seaside | 39 MB | 24,316 | 23 | 1.4 s | 620 MB | 1.9 s | 0.1 s | 0.10 s | 3.8 s |
| cruise | 181 MB | 25,270 | 47 | 2.9 s | 909 MB | 1.4 s | 0.8 s | 0.32 s | 4.9 s |
| models | 26 MB | 652 | 168 | 1.0 s | 567 MB | 1.4 s | 0.07 s | 0.10 s | 2.3 s |

Materials are one per JS material drawn: instance colours ride in the tint
shader (D103); a material per distinct colour had given Sierra 1,838 and
Streets 1,580. Web "first frame" is the client's first rendered frame,
before the scene arrives (G1's load-time measure); "ready" is the scene up
with every pipeline compiled. Ranges are over the dev and release builds and
repeated runs. The dev wasm is 77 MB raw.

**Release build** (`cargo xtask web --release`, fat LTO, `wasm-opt -Oz`;
3 min 14 s at load average about 30): `mr_game_bg.wasm` 18.91 MB, **5.87 MB
after gzip** (G1's limit: 10 MB). Flying Sierra and Coast at 60 m/s from
s = 80 in headless Chrome (WebGPU, 1280 × 800), shadows off and on: a steady
60 fps (the display rate) with no frame over 16.8 ms after the first second
of flight; Coast with shadows had one 217 ms frame just after "ready" (a
pipeline compiled late). Phones not measured yet.

## The Rust client at WP 2.3 (three's shading, sky, environment, shadow, post)

2026-10-03, desktop (RTX 3060, Linux, load average 15 to 30 from other
work), JS tree key `2a65261c9506d9cd`. The metric is SPEC 12's (CIEDE2000 on
quarter resolution; a picture passes with mean under 3 and the 95th
percentile of 16-pixel blocks under 6; D18).

**Material test scenes** (`cargo xtask parity materials`, the gate of
roadmap WP 2.3; JS run `a` against the native client, 512 × 512): all
thirteen within the limits, by a wide margin.

| Scene | Mean ΔE00 | Block 95 % |
|---|---:|---:|
| fixed/bloom-chart | 0.016 | 0.025 |
| fixed/fog-ramp | 0.023 | 0.115 |
| fixed/shadow-edge | 0.150 | 0.337 |
| fixed/standard-metal-0 | 0.087 | 0.456 |
| fixed/standard-metal-0.33 | 0.102 | 0.460 |
| fixed/standard-metal-0.66 | 0.109 | 0.460 |
| fixed/standard-metal-1 | 0.112 | 0.459 |
| fixed/physical-clearcoat-sheen | 0.137 | 0.609 |
| kinds/kind-Standard | 0.107 | 0.348 |
| kinds/kind-Physical | 0.150 | 0.391 |
| kinds/kind-Lambert | 0.019 | 0.091 |
| kinds/kind-Basic | 0.001 | 0.006 |
| kinds/kind-SkyDome | 0.032 | 0.235 |

The largest single-pixel differences (9 to 19 ΔE00) sit on sphere
silhouettes and specular highlights (MSAA resolve and the GPU's
derivatives), not in areas. For the record, the other kinds drawn as their
plain stand-ins (`--only every`, not a gate): within the limits already
Reflector 0.14, Siding 0.11, Shoulder 0.42, Stucco 0.55, Sea 0.62,
AmbientProp 0.90, Asphalt 1.20, Neon 2.78 (block 5.8); over them Markings
4.1, FloodBeam 9.2, Terrain 9.5, Sandstone 11.3, CityFacade 12.7,
TriplanarRock 13.5, CarLight 17.9, StreetFacade 18.2, ContainerAtlas 21.9,
GroundPool 21.9, StreetAtlas 25.9 (their patches are WP 2.4 and M3). Lines,
points, sprites and the effect shaders are not rendered by the Rust side
yet.

**Screenshot stations** (`cargo xtask parity stations`, D17's 398 stations
against JS run `a`, the native client at 1280 × 800, frozen scenery). Most
kinds are still stand-ins, so this is a progress measure, not a gate:

| Level | Stations | Within the limits | Median mean | Worst mean | Median block 95 % |
|---|---:|---:|---:|---:|---:|
| sierra | 81 | 13 | 4.27 | 14.80 | 11.41 |
| coast | 67 | 21 | 2.23 | 8.36 | 8.52 |
| streets | 43 | 0 | 11.01 | 13.12 | 34.20 |
| desert | 63 | 9 | 4.26 | 12.17 | 9.50 |
| seaside | 29 | 2 | 3.38 | 13.07 | 18.39 |
| cruise | 115 | 3 | 4.13 | 7.52 | 18.88 |

The WP 2.2 client (Bevy's `StandardMaterial` stand-ins, Bevy's bloom and
tone mapping, the export's sky) on Sierra's 81 stations: none within the
limits, median mean 21.1, median block 33.9. What is left is the patched
kinds (terrain strata, road wear and night sheen, city façades and lit
windows, ground pools, glow points, sea foam): with the sky, clouds, sun
disc, shadows, fog and exposure the same, daytime stations with little
patched surface already pass (Sierra 1250 to 5750 chase, much of Coast).
Rust and JS side by side for a few stations:
`parity/report/wp23-side-by-side/` on the registered server (made from
`parity/cache/<key>/shots/rust/` and `…/a/`).

**Timings.** `cargo xtask parity materials`: 7 s with both sides cached (the
native client renders the 13 scenes in about 4 s). `cargo xtask parity
stations`: 2 min 52 s for all six levels (Rust side only; the JS shots were
cached). Native start to ready with the new pipelines, Sierra: 3.0 s, peak
RSS 905 MB (WP 2.2: 4.1 s, 839 MB; the environment atlas, post targets and
pipelines add about 70 MB). The environment map (28 passes) is rebuilt
every 2.5 % of a sprint route.

**Web.** Release build (`cargo xtask web --release`): `mr_game_bg.wasm`
20.2 MB, 6.26 MB after gzip (WP 2.2: 5.87 MB). In headless Chrome on WebGPU
(`tools/parity/rust-web.mjs`), Seaside ready 4.2 to 9.7 s after navigation (models 3.4 s) with
the dev machine loaded; the WGSL compiles in Chrome's compiler and the
screenshot matches the native one. Frame times on the phones: WP 2.6.

## The Rust client at WP 2.4 (terrain, road, sea and points kinds)

2026-10-03, desktop (RTX 3060, Linux, load average 30 to 50 from other
work), JS tree key `2a65261c9506d9cd`, the metric as in the WP 2.3 section.

**The gate** (`cargo xtask parity stations --base`): Sierra's
terrain-road-sky export (`sierra.base.mrscene`, native client, 1280 × 800)
against the JS game with only the terrain, road and sky drawn
(`tools/parity/base-shots.mjs`, DECISIONS D292), at the five stations named
in advance:

| Station | Mean ΔE00 | Block 95 % |
|---|---:|---:|
| sierra/attract | 0.171 | 0.294 |
| sierra/02000-chase | 0.159 | 0.321 |
| sierra/04500-high | 0.149 | 0.262 |
| sierra/07000-chase | 0.134 | 0.283 |
| sierra/09500-high | 0.134 | 0.223 |

Every base station of every level is within the limits too (progress, not
gate; Seaside's include its draped photo):

| Level | Stations | Within the limits | Median mean | Worst mean | Median block 95 % |
|---|---:|---:|---:|---:|---:|
| sierra | 81 | 81 | 0.15 | 0.21 | 0.28 |
| coast | 67 | 67 | 0.15 | 0.21 | 0.32 |
| streets | 43 | 43 | 0.11 | 0.13 | 0.21 |
| desert | 63 | 63 | 0.12 | 0.15 | 0.23 |
| seaside | 29 | 29 | 0.21 | 0.37 | 0.47 |
| cruise | 115 | 115 | 0.14 | 0.17 | 0.26 |

**Material test scenes** (`cargo xtask parity materials`, now including
the WP 2.4 kinds): all eighteen within the limits; the new ones are
Terrain 0.053 / 0.126, Asphalt 0.089 / 0.174, Shoulder 0.082 / 0.162,
Markings 0.078 / 0.233, Sea 0.072 / 0.191 (mean / block 95 %); the WP 2.3
scenes are unchanged (largest Physical and shadow-edge, 0.150). As
stand-ins at WP 2.3 these five were at 9.5, 1.20, 0.42, 4.1 and 0.62 mean.
`--only every`: the kinds still drawn as stand-ins are as before
(StreetAtlas 25.9 down to Stucco 0.55).

**Screenshot stations, full scenes** (`cargo xtask parity stations`, 398
stations; progress, not gate; WP 2.3 in brackets):

| Level | Stations | Within the limits | Median mean | Worst mean | Median block 95 % |
|---|---:|---:|---:|---:|---:|
| sierra | 81 | 48 (13) | 0.44 (4.27) | 14.79 | 1.28 (11.41) |
| coast | 67 | 43 (21) | 0.87 (2.23) | 5.66 | 3.33 (8.52) |
| streets | 43 | 0 (0) | 10.85 (11.01) | 12.98 | 34.20 (34.20) |
| desert | 63 | 52 (9) | 0.27 (4.26) | 6.58 | 0.65 (9.50) |
| seaside | 29 | 29 (2) | 0.20 (3.38) | 0.42 | 0.49 (18.39) |
| cruise | 115 | 3 (3) | 3.79 (4.13) | 7.29 | 18.82 (18.88) |

175 of 398 within the limits (WP 2.3: 48). What is left is the kinds of M3:
the city and street façades and atlases (lit windows), the container
atlas, sandstone and triplanar rock, the effect shaders (sky glow, traffic
streams, surf, beams) and the animators (lamp pools and colours that follow
the night). Side by side, the five gate stations, the same five in the full
scene and four from other levels: `parity/report/wp24-side-by-side/` (from
the worktree that ran them).

**Web.** Release build: `mr_game_bg.wasm` 20.4 MB, 6.35 MB after gzip
(WP 2.3: 6.26 MB). In headless Chrome on WebGPU (`rust-web.mjs`), scenes
cut down to the new kinds (sea, terrain, road and points of Coast; glow
points of Sierra; flicker points of Desert) compile and draw without
errors, ready 2.2 to 3.1 s after navigation.

**Timings.** `cargo xtask parity stations --base` on Sierra: 74 s with the
JS shots cached (81 Rust stations).

## Owner's phone check (G1, first look)

2026-10-03: the owner opened the Bevy web build (WP 2.1–2.3, WebGPU) on
their iPhone over the tailnet and reported that it "looked great". No
numbers were taken; the phone model and iOS version are not recorded yet.
The rest of G1 (frame time against the JS fly-camera baseline, ten minutes
and ten reloads without a tab kill, no frame over 50 ms after warm-up) waits
for WP 2.6's warm-up and the measurement pages.

## The Rust client at WP 2.6 and 2.7 (warm-up, frame time, memory, reloads, WebGL2)

2026-10-03, desktop: RTX 3060 shared with other work (other agents' headless
Chrome runs and a resident speech-to-text process; load average 3 to 19 on
24 cores, given per run), Chrome 151 headless on Linux, release builds,
1280 × 800 at device pixel ratio 1, high quality on, frame rate uncapped
(no vsync, no frame-rate limit) as `tools/parity/perf-baseline.mjs`
measures the JS game. Flights are the fly camera at 60 m/s from s = 80 for
the JS baseline's length (Sierra 170 s, Coast 138 s). Rust numbers are the
measurement page's (`index.html?perf=1`, DECISIONS D395) driven by
`node tools/parity/rust-perf.mjs`; the JS rows are the JS game flown the
same way with the same requestAnimationFrame recorder (`--game js`), in
between the Rust runs. Frame times are ms; fps is per half-second window
(median and 5th percentile, as the JS baseline); "> 50 ms" counts frames
after the warm-up, in the first 30 s and in the whole flight.

| Build | Level | Load avg | fps median / 5th | Frame ms p50 / p95 / p99 / max | > 50 ms (30 s / all) | Ready | First frame |
|---|---|---:|---:|---:|---:|---:|---:|
| JS (three.js, WebGL2) | sierra | 18.5 | 720.7 / 423.2 | 0.6 / 2.5 / 18.3 / 173.2 | 4 / 256 | 3.6 s (menu 3.2 s) | |
| JS | sierra | 14.8 | 632.0 / 357.4 | 0.6 / 4.2 / 18.4 / 331.2 | 4 / 239 | 6.1 s | |
| JS | coast | 3.0 | 581.4 / 316.9 | 0.8 / 6.6 / 17.0 / 121.3 | 4 / 36 | 3.5 s | |
| Rust, WebGPU | sierra | 11.0 | 55.1 / 17.0 | 7.4 / 33.9 / 78.2 / 194.2 | 7 / 281 | 3.0 s | 1.13 s |
| Rust, WebGPU | sierra | 10.7 | 69.8 / 39.3 | 6.0 / 25.0 / 34.3 / 93.5 | 3 / 29 | 2.9 s | 1.07 s |
| Rust, WebGPU, hq off | sierra | 5.1 | 129.1 / 30.3 | 5.8 / 22.8 / 36.2 / 102.2 | 9 / 78 | 2.8 s | 1.05 s |
| Rust, WebGPU | coast | 4.2 | 64.1 / 47.8 | 4.5 / 32.1 / 34.1 / 188.0 | 42 / 96 | 2.9 s | 0.97 s |
| Rust, WebGPU | coast | 19.1 | 54.7 / 31.9 | 4.4 / 23.6 / 34.0 / 246.4 | 33 / 64 | 5.5 s | 1.84 s |
| Rust, WebGL2 | sierra | 6.5 | 97.5 / 46.6 | 6.2 / 20.3 / 22.9 / 39.6 | 0 / 0 | 3.8 s | 1.66 s |
| Rust, WebGL2 | coast | 3.9 | 91.7 / 72.4 | 6.0 / 13.9 / 16.1 / 38.9 | 0 / 0 | 3.5 s | 1.64 s |

The WP 0.8 desktop baseline above (fps median / 5th: Sierra 507 / 230,
Coast 459 / 300) was taken the same way at load average about 20.

**Warm-up (SPEC 6.3; D390).** Sierra draws 45 material × mesh-layout
combinations, Coast 43, Seaside 14, the models 11; they compile behind
the loading screen, and in every run above no pipeline was compiled after
it (`__mr.lateFrames` 0). Before the GPU fence the first flight frames had
one frame of about 1.07 s just after "ready" (the browser finishing the
pipelines in its GPU process); with it, none. The JS game's own worst
frames include its shader compiles (the "> 50 ms" frames in its first
seconds and its 121 to 331 ms maxima).

**Gate "no frame over 50 ms after warm-up in the first thirty seconds":
not met on WebGPU, met on WebGL2.** The WebGPU frames over 50 ms are not
pipeline compiles. They come back at the same places on every run (Sierra
s ≈ 955 to 975 and ≈ 1,470 to 1,490, the hairpins; Coast s ≈ 355 to 400,
600 to 615, 1,015, and its first 4 km run at 20 ms a frame against 4 ms
after), with or without the stand-in cars, the shadows, or the time of day
moving. A CPU profile across Coast s 350 to 405 puts 60 % of the main
thread in Chrome's `writeBuffer`, called from Bevy's GPU-preprocessing
uploads (`write_batched_instance_buffers`, `write_mesh_culling_data_buffer`),
whose size follows the entity count; across Sierra's hairpin the time is
spread over Bevy's per-entity work (visibility, extraction, sorting,
uploads). The client makes one entity per instance of an `InstancedMesh`
(DECISIONS D101): Sierra 63,670 entities, Coast 28,673, where the JS game
issues about 800 draw calls (SPEC 6.6: 833 at peak on Sierra). WebGL2, where Bevy has no GPU preprocessing,
shows none of these frames but a higher median. Turning GPU preprocessing
off on WebGPU (`?gpupre=0`, D396) lowers the worst frames and raises the
median (Coast, first 40 s, alternating runs at load 5 to 10: on, p50 18 ms,
max 260 to 408 ms, 31 to 33 frames over 50 ms; off, p50 31 to 32 ms, max
96 to 128 ms, 48 to 165 over 50 ms). **Frame time against the JS game:
the Rust client is about ten times slower per frame on this desktop**
(median 4.4 to 7.4 ms against 0.6 to 0.8 ms). It is CPU-bound on
per-entity work. The fix this points to is a renderer change: one entity
per `InstancedMesh` with a per-instance buffer, as three draws it (a few
hundred entities instead of tens of thousands). That is the main finding
for gate G1: measure on the phones before deciding (below).

**Memory, ten reloads (SPEC 6.6; D394).** The wasm memory's size, which is
its high-water mark, in MB, after the first load and after each reload
(`__mr.reload`, the scene torn down and loaded again in place):

| Run | After load | Reloads 1 to 10 |
|---|---:|---|
| Sierra, ten times Sierra (two runs, the same) | 299 | 500, 509, 641, 641, 641, 641, 641, 641, 641, 641 |
| Coast, then Coast and Sierra in turn | 436 | 636, 636, 656, 788, 788, 788, 988, 988, 988, 988 |
| the same, second run | 436 | 528, 528, 728, 728, 928, 928, 928, 928, 928, 928 |
| WebGL2: Sierra, then Sierra twice | 338 | 494, 625 |

No leak: Sierra reloading Sierra stops growing at the third reload, and the
level switches stop at the fifth to seventh. But the high-water mark is
above SPEC 6.6's 512 MB phone budget after one Coast load plus a switch,
and after Sierra's third reload: each load copies the whole export into
the wasm (131 MB for Sierra, 200 MB for Coast), parses it, then builds
the scene, and the allocator does not find the old space contiguous. The
JS heap holds up to 400 MB of downloaded scene buffers between garbage
collections. Loading a multi-hundred-megabyte export is not how the
finished game gets its world (G1 says so), but the phone may still kill
the tab here: the owner's reload run will tell. Reload times: 1.1 to 3.1 s
(Sierra), 1.3 to 2.0 s (Coast) from the request to running again, the
pipelines already compiled; worst frame in the 3 s after each reload 9 to
97 ms.

**Load (G1: first rendered frame within 5 s of the JS game's time to its
menu).** First frame 0.97 to 1.84 s after navigation (WebGPU), 1.64 to
1.66 s (WebGL2); ready, with the scene downloaded from this machine,
built and warmed up, 2.8 to 5.5 s. The JS game reaches its menu in 3.2 s.
Met with room.

**Size.** `cargo xtask web --release`: `mr_game_bg.wasm` 19.58 MB, 6.09 MB
after gzip (WebGPU); `mr_game_webgl2_bg.wasm` 20.81 MB, 6.56 MB after gzip
(WebGL2). G1's limit is 10 MB.

**WebGL2 (WP 2.7).** The page loads the WebGL2 build when the browser
gives no WebGPU adapter (`tools/parity/rust-web.mjs --backend webgl2`
starts Chrome with WebGPU turned off). It renders the same pictures:
against the WebGPU build, Seaside at three views (one under the start
bridge's shadow, one from above) and the models scene differ by 0.001
ΔE00 mean at most, and WP 2.4's five gate stations of Sierra's base export
by 0.000; against the JS game those five stations are at 0.13 to 0.17
mean, 0.22 to 0.32 block 95 % on both builds, the same as the native
client at WP 2.4. Shadows, bloom, the environment map, fog and the
patched kinds all draw. Two shader changes were needed (D393). The WP 2.3
material scenes are unchanged natively (all eighteen within the limits,
largest 0.150 mean).

**How to run.** `cargo xtask web --release`, then (registered server up)
`node tools/parity/rust-perf.mjs --level sierra` (WebGPU, ten reloads),
`--level coast --reload-levels coast,sierra`, `--backend webgl2`,
`--hq 0`, `--secs N`, `--query "gpupre=0"`; `--game js` for the JS game.
Results print, and land in `parity/report/perf/`.

### Phones (the owner runs these)

Tailnet https address of the dev server plus the path below, after
`cargo xtask web --release` in the checkout the server serves. The page
flies Sierra or Coast at 60 m/s, then reloads ten times, and shows the
numbers on the screen (about 5 minutes; keep the screen on; "Copy
results" puts them on the clipboard). High quality is off by default on
a phone, as G1 asks.

| What | Path |
|---|---|
| Sierra, WebGPU | `dist/next/?level=sierra&perf=1` |
| Coast, WebGPU | `dist/next/?level=coast&perf=1` |
| Sierra, level switches (memory) | `dist/next/?level=sierra&perf=1&reloadLevels=sierra,coast` |
| Sierra, WebGL2 build | `dist/next/?level=sierra&perf=1&backend=webgl2` |
| Sierra, GPU preprocessing off | `dist/next/?level=sierra&perf=1&gpupre=0` |
| Quick look (flight only) | `dist/next/?level=sierra&perf=1&reloads=0&secs=60` |

| Device | Build | Level | fps median / 5th | Frame ms p50 / p95 / p99 / max | > 50 ms (30 s / all) | First frame | Ready | wasm MB after load → reload 10 | Notes |
|---|---|---|---:|---:|---:|---:|---:|---:|---|
| iPhone | WebGPU | sierra | | | | | | | |
| iPhone (Safari 26.6.1, which reports itself as a Mac; 1320×2388 @ dpr 3, portrait) | WebGPU (Apple adapter) | seaside, quick look, hq off | 60 / 59.9 | 17 / 17 / 17 / 72 | 2 / 2 | 1.05 s | 10.47 s (37.5 MB scene downloaded in 3.7 s over the tailnet; 14 warm-up pipelines) | 168 → 168 (no reloads) | 2026-10-04, build 9b573da. Slow frames 72 ms at s 648 and 51 ms at s 1458, nothing else over 21 ms; no stall near s 3100 to 3300, unlike both Firefox runs |
| iPhone | WebGPU | coast | | | | | | | |
| iPhone | WebGL2 | sierra | | | | | | | |
| Owner's Linux desktop, Firefox 154 | WebGL2 (picked by itself: no WebGPU) | seaside, quick look, hq on, 3373×1323 @ dpr 1 | 60 / 52 (vsync) | 17.1 / 17.1 / 33.1 / 532.8 | 1 / 2 | 0.34 s | 1.37 s (37.5 MB scene in 0.2 s; 14 warm-up pipelines) | 159 → 159 (no reloads) | 2026-10-04, build 9b573da. The 50.2 ms frame is at 0.2 s, just after ready; the 532.8 ms one at 53.6 s, s 3272, with no pipeline compiled after warm-up |
| Owner's Linux desktop, Firefox 154 with WebGPU turned on | WebGPU | seaside, quick look, hq on, 3373×1323 @ dpr 1 | 60 / 55.8 (vsync) | 17 / 30 / 32 / 458 | 3 / 6 | 3.49 s | 5.00 s (37.5 MB scene in 0.1 s; 14 warm-up pipelines) | 176 → 176 (no reloads) | 2026-10-04, build 9b573da, three minutes after the WebGL2 row. Slow frames: 96 ms at s 463, then 55 to 57 ms every 9 s or so (s 1329, 1845, 2401, 2920), and 458 ms at 51.4 s, s 3137, close to the WebGL2 run's 533 ms at s 3272: the same stretch of route on both backends |
