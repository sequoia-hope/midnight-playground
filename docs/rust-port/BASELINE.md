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
9900X). The Rust figure is `cargo run --release -p mp_sim --bin mp-sim --
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
indications, not baselines. Native: the dev build (`cargo run -p mp_game`,
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
3 min 14 s at load average about 30): `mp_game_bg.wasm` 18.91 MB, **5.87 MB
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

**Web.** Release build (`cargo xtask web --release`): `mp_game_bg.wasm`
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

**Web.** Release build: `mp_game_bg.wasm` 20.4 MB, 6.35 MB after gzip
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
it (`__mp.lateFrames` 0). Before the GPU fence the first flight frames had
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
(`__mp.reload`, the scene torn down and loaded again in place):

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

**Size.** `cargo xtask web --release`: `mp_game_bg.wasm` 19.58 MB, 6.09 MB
after gzip (WebGPU); `mp_game_webgl2_bg.wasm` 20.81 MB, 6.56 MB after gzip
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
| Same desktop, Firefox 154 WebGPU, repeat with no setting changed (`allow-present-without-readback` was already true) | WebGPU | seaside, quick look, hq on, 3373×1323 @ dpr 1 | 59.9 / 46.5 | 17.1 / 17.5 / 34 / 83.3 | 3 / 7 | 0.61 s | 1.41 s | 176 → 176 | 2026-10-04 08:59, build 9b573da. No long stall this time (worst 83 ms at s 2818); the slow frames are 50.2 ms, three vsyncs, at s 1063 to 2585. The first run's 3.5 s to first frame and 458 ms stall did not repeat, so they were likely a cold start (first WebGPU use after turning it on) and something not tied to the route |

## The Rust client after the instancing fix (DECISIONS D450 to D456)

2026-10-04, the same desktop and method as the WP 2.6 section above (RTX
3060 shared with other work, Chrome headless, release builds, 1280 × 800,
dpr 1, high quality, uncapped, `node tools/parity/rust-perf.mjs`; the JS
rows flown the same way between the Rust runs). Load average is the
1-minute figure at the start of each run.

**Entities** (`scene built: … entities`): an `InstancedMesh` is now one
entity per material group, drawn with one instanced draw (D450).

| Level | Before (WP 2.6) | After |
|---|---:|---:|
| Sierra | 63,670 | 508 |
| Coast | 28,673 | 436 |
| Seaside | 24,316 | 220 |
| Desert | 19,117 | 363 |
| Streets | 15,217 | 433 |
| Cruise | 25,270 | 432 |

**Flights** (WP 2.6's rows for comparison are in the table above). "Inst."
is the instancing alone (D450 to D454); "final" adds D455 (the night
materials in the shader).

| Build | Level | Load avg | fps median / 5th | Frame ms p50 / p95 / p99 / max | > 50 ms (30 s / all) | Ready | First frame |
|---|---|---:|---:|---:|---:|---:|---:|
| JS (three.js, WebGL2) | sierra | 2.7 | 729.5 / 458.1 | 0.5 / 1.9 / 18.3 / 154.7 | 2 / 275 | 4.2 s (menu 3.2 s) | |
| JS | coast | 2.1 | 595.6 / 455.6 | 0.7 / 6.9 / 17.5 / 121.0 | 6 / 39 | 2.8 s | |
| Rust, WebGPU, inst. | sierra | 1.9 | 211.8 / 61.6 | 5.0 / 15.3 / 24.5 / 46.6 | 0 / 0 | 3.0 s | 1.09 s |
| Rust, WebGPU, inst. | coast | 2.5 | 66.7 / 55.7 | 4.4 / 19.9 / 33.4 / 64.8 | 10 / 14 | 4.1 s | 2.15 s |
| Rust, WebGPU, final | sierra | 7.3 | 251.6 / 212.7 | 2.6 / 9.4 / 11.4 / 27.5 | 0 / 0 | 3.5 s | 1.11 s |
| Rust, WebGPU, final | coast | 3.4 | 265.5 / 209.7 | 3.2 / 8.3 / 10.3 / 29.9 | 0 / 0 | 4.7 s | 2.62 s |
| Rust, WebGL2, final | sierra | 2.0 | 265.6 / 143.4 | 1.3 / 18.5 / 19.1 / 21.3 | 0 / 0 | 3.4 s | 1.68 s |
| Rust, WebGL2, final | coast | 2.3 | 99.5 / 89.9 | 1.8 / 36.2 / 53.1 / 54.1 | 10 / 643 | 3.4 s | 1.66 s |

**Gate "no frame over 50 ms after warm-up in the first thirty seconds":
met on WebGPU** (Sierra and Coast, 0 in the first 30 s and 0 in the whole
flight; worst frames 27.5 and 29.9 ms against 194 to 408 ms at WP 2.6).
No pipeline was compiled after the warm-up in any run. The median frame on
WebGPU went from 4.4 to 7.4 ms to 2.6 to 3.2 ms; the JS game's is 0.5 to
0.7 ms, so the Rust client is still about 4 to 5 times the JS per frame by
this clock (the JS figure is its main thread's time, as before).

What the instancing alone left (the "inst." rows): Coast's first 4 km and
Sierra's 3 to 7 km ran at 15 to 16 ms a frame with 33 and 50 ms frames,
only while the time of day moved (pinned with `?t=`, Coast's first 40 s:
2.7 ms median, 22 ms worst). A Chrome trace there showed Chrome's GPU
process decoding the page's WebGPU commands all the time and the page
waiting on it 80 % of the flight; the cause was the night-following
materials being edited every frame (Bevy re-prepares an edited material).
D455 moves that into the shader: Coast's first 40 s went to 2.6 ms median.

**WebGL2 on Coast** is limited by the GPU side at about 100 frames a second
on this desktop (the WP 2.6 build ran at 92). With the CPU work now short,
the uncapped page queues frames in bursts of 1 to 4 ms and then waits
36 or 53 ms for the GPU (two or three of Chrome's 17.7 ms intervals),
hence the 643 frames over 50 ms; throughput is the same as before. With
vsync on (`--capped`, as a browser runs it), Coast's first 60 s on WebGL2
hold 60 frames a second with no frame over 16.8 ms. Sierra on WebGL2 has
none of this (265 fps).

**Memory, ten reloads** (wasm memory size, its high-water mark, MB):

| Run | After load | Reloads 1 to 10 | WP 2.6 |
|---|---:|---|---|
| Sierra, ten times Sierra (WebGPU) | 303 | 312, 443 × 9 | 299 → 641 |
| Coast, then Coast and Sierra in turn (WebGPU) | 440 | 448, 579 × 9 | 436 → 928 to 988 |
| Sierra, ten times Sierra (WebGL2) | 294 | 425 × 10 | 338 → 625 (2 reloads) |
| Coast, ten times Coast (WebGL2) | 431 | 630 × 10 | |

No growth after the second reload in any run. The high-water mark is 200
to 400 MB lower; Sierra stays under SPEC 6.6's 512 MB phone budget, Coast's
switches (579 MB) and Coast reloading itself on WebGL2 (630 MB) do not.
What is left is the scene file copied into the wasm whole (131 MB Sierra,
200 MB Coast) and parsed beside the old one's freed space; how scenes are
delivered is the open question of DECISIONS D439, not changed here.

**Pictures.** `cargo xtask parity materials`: 18 stations, 0 over the
limits, worst 0.150 mean (unchanged). WP 2.4's five Sierra gate stations
through the web build at 1280 × 800 (`tools/parity/rust-web.mjs`, base
export): WebGPU and WebGL2 alike attract 0.171 / 0.294, 02000-chase
0.158 / 0.321, 04500-high 0.143 / 0.258, 07000-chase 0.133 / 0.282,
09500-high 0.131 / 0.219 (mean / block 95 %), the same as WP 2.4 or a
little better. Seaside's full export through the web build (all 29
stations, 1280 × 800): 29 within the limits, median 0.194 / 0.486, worst
0.418 / 0.851 (WP 2.4 native: median 0.21, worst 0.42), WebGL2 against
WebGPU 0.001. The 398 full-scene stations natively (1024 × 701, the X
display's size) before and after: the same to 2 levels of 255 except the
stations of D452 (Streets' lamp globes, whose instance colours above 2
are no longer clamped) and a few edge pixels (D451).

**Size.** `mp_game_bg.wasm` 27.59 MB, 8.77 MB after gzip;
`mp_game_webgl2_bg.wasm` 28.83 MB, 9.24 MB after gzip (budget 10 MB).
The build this change started from (9b573da) is 27.51 / 8.74 MB and
28.75 / 9.22 MB: this change adds 0.08 MB, 0.02 to 0.03 MB after gzip.
The growth since the WP 2.6 figures above (6.09 / 6.56 MB after gzip) came
with M3's and M4's code (world generation and the car models in the
client), merged before 9b573da.

**Races at dusk (D456).** The race with the autopilot (seed 1, high
quality, uncapped, 60 s from the start of racing; Coast at `timescale=2`
over s 40 to 6,100 on WebGPU and to 4,500 on WebGL2, its dusk; Sierra at
`timescale=3` over s 40 to 8,100, through its dusk at 3 to 7 km), the
build before D456 (instancing and D455) and after it, run alternately,
two rounds (load average 2.7 to 5.8). Frames over 50 ms, then p95 and the
worst frame in ms:

| Level, backend | Before D456 | After D456 |
|---|---|---|
| Coast, WebGPU | 3, 16; p95 15.3, 16.3; max 82.8, 80.7 | 1, 0; p95 9.0, 9.1; max 52.6, 48.8 |
| Sierra, WebGPU | 1, 4; p95 11.5, 12.1; max 117.9, 208.2 | 1, 0; p95 10.2, 8.1; max 50.2, 37.8 |
| Coast, WebGL2 | 8, 0; p95 39.1, 37.4; max 55.8, 40.0 | 47, 12; p95 37.4, 37.6; max 56.6, 55.3 |
| Sierra, WebGL2 | 0, 0; p95 19.8, 19.7; max 43.5, 37.7 | 0, 0; p95 19.7, 19.7; max 37.0, 38.4 |

On WebGPU the light setters' per-frame material edits were the slow
frames at dusk; after D456 the one frame over 50 ms left is at the start
of the race (s 43, 52.6 ms) or at Sierra s 5,137 (50.2 ms), where the race
compiles pipelines the warm-up does not cover: every race run, before and
after, reports `__mp.lateFrames` 1 (Coast) or 3 (Sierra). The race's car
models (D440) are spawned when the race starts and are not part of the
warm-up's combinations (D390), which is the next thing to fix for SPEC
6.3's gate in a race (play/ is not changed here beyond D456). WebGL2 on
Coast is the GPU-bound pacing described above, in both builds alike.

**Seaside in headless Chrome (the owner's Firefox stall at s 3,100 to
3,300).** The same quick-look flight with this build (60 s from s 80,
high quality, reloads 0) covers s 80 to 3,680: on WebGL2 uncapped p50 1.0
ms, worst 36.1; on WebGPU uncapped p50 1.8 ms, worst 29.8; capped at 60 Hz
on both backends, and from s 1,500 on both, every frame 16.7 to 16.8 ms.
No frame over 50 ms anywhere, and no periodic slow frames: Chrome does not
show the stall or the 9-second frames the Firefox runs had (nor did the
iPhone). The Firefox investigation follows separately.

## Firefox (the owner's browser): headless runs on the dev machine

2026-10-04, Firefox 156.0.1 (snap) headless, driven by puppeteer over
WebDriver BiDi (`browser: 'firefox'`; the profile under `~/snap/firefox/
common/` because the snap cannot see `/tmp`), WebGPU with
`dom.webgpu.enabled` and `gfx.webgpu.ignore-blocklist`, through the
registered server, 1280 × 800, high quality, the measurement page's quick
look on Seaside (s 80, 60 m/s), vsync on as Firefox runs it. "Base" is the
build before the instancing change (9b573da), "after" the instancing
change. The machine was shared (load 3 to 9; one later stretch at 26 to
31, marked).

| Build | Backend | Flight | > 33 / > 50 ms | p95 | Worst |
|---|---|---|---:|---:|---:|
| base | WebGL2 | 60 s, twice | 0, 15 / 0, 0 | 17.1 | 17.6, 34.2 |
| after | WebGL2 | 60 s, twice | 0, 0 / 0, 0 | 17.1 | 17.5, 17.5 |
| base | WebGPU | 60 s, twice | 61, 125 / 11, 15 | 32.6, 33.3 | 116.4, 100.7 |
| after | WebGPU | 60 s, twice (load 8.6, 8.9) | 8, 445 / 4, 75 | 17.1, 34.3 | 183.3, 233.8 |
| after | WebGPU | 30 s, nine runs at load 2.7 to 4.1 | 0 to 4 / 0 to 2 | 17.1 | 33 to 83 |

**The stall near s 3,100 to 3,300 was not reproduced**: no WebGL2 run had a
frame over 34 ms over the whole route (s 80 to 3,680), and no WebGPU run
had one over 233 ms. In one profiled WebGPU run at load 26 a major GC took
471 ms of wall time across its slices: a stall of that size fits a GC
under load, but that is not shown for the owner's runs.

**The 33 and 50 ms frames on WebGPU follow time, not the route.** A frame
of 50.2 ms comes back at s 1,362 to 1,378 from s 80 and at s 1,803 to 1,812
from s 500: both about 21.5 to 22 s into the flight. A Gecko profile
(`MOZ_PROFILER_STARTUP`) shows the page's own work short (the
`requestAnimationFrame` callback p50 3.0 ms, p99 7.1, worst 13.8) and the
long frames are waits: one comes right after a major GC in the content
process (reason TOO_MUCH_MALLOC, 16.8 ms across four slices, at 23 s), the
others with the compositor presenting the previous WebGPU frame late
(`CONTENT_FRAME_TIME` 88 ms against 30 ms usually) and no GC. Minor GCs run
17 times a second (0.1 to 0.4 ms each, harmless). The major GC is driven
by the native memory behind the WebGPU objects the page creates every
frame: per frame on Seaside, 41 bind groups, 6 buffers (5 mapped at
creation), 8 command encoders, 18 render passes and one buffer mapped for
reading (counted in Chrome by wrapping the `GPU*` prototypes; `writeBuffer`
traffic 717 KB a frame before the instancing change, 34 KB after). D457
removes the post chain's share (13 bind groups and a buffer); in the one
profiled run since, the next TOO_MUCH_MALLOC GC came at 42 s instead of 23
(that run was at load 26, so its frame times are not comparable). The
rest is Bevy's per-frame bind groups and buffers.

**Prefs.** `dom.webgpu.allow-present-without-readback` (it exists in this
build): false, true and the default gave the same results, three 30 s
runs each (frames over 50 ms 2, 1, 1 / 0, 1, 1 / 1, 0, 1; p95 17.1 in all).
No pref tested here changed the results measurably. The first frame on
WebGPU here was 1.5 and 2.0 s (at load 28; WebGL2 0.5 s at load 3); the
owner's 3.5 s was a cold first run (his repeat: 0.61 s).

**How to run.** The scripts are in the session's scratchpad, not the repo:
`ff.mjs` (one flight: dist, backend, level, seconds, s0, prefs JSON),
`ffprof.sh` (the same under the Gecko profiler), `gcstat.py` and `gap.py`
(frames, GCs and markers from the profile), `calls.mjs` (WebGPU calls per
frame in Chrome).

**Race starts (D458, D459).** Races with the autopilot (seed 1,
`timescale=2`, high quality, 10 s from the start of racing, three rounds,
uncapped; load 10 to 26, the machine busy with other work): late
pipelines 0 in every run on Coast and Sierra and both backends (were 1 and
3). On WebGPU no frame over 50 ms in the start (s 42 to 50) in any of the
six runs (before D459: 50 to 132 ms frames there in four of six); single
stalls of 730 to 880 ms elsewhere in three runs at load 18 to 26 (s 108,
678, 376), not seen at lower load. On WebGL2, Sierra none; Coast is
GPU-bound from the start (56 to 77 ms intervals uncapped, as in the
flights above), and with vsync on (`CAPPED=1`) holds 30 to 60 frames a
second with no frame over 33.5 ms; WebGPU on Coast with vsync on holds 60
with no frame over 16.8 ms.

At lower load afterwards (30 s races, `timescale=2`, uncapped): WebGPU,
Coast at load 4.7 and 8.9: 0 frames over 50 ms, worst 32.5 and 49.1 ms;
Sierra at 8.7: 0, worst 24.8 ms; Sierra at 14.6: 2 (54 ms at s 67, 60 ms
at s 1,899), worst 60.1 ms. No stall of the 730 to 880 ms kind in these
four runs: they look like the busy machine, but that is not confirmed.
WebGL2 at load 8.2 and 7.0: Coast 259 frames over 50 ms (73 to 76 ms
intervals from the start, worst 123.6), Sierra 12 (54 to 57 ms, one of
106 ms at s 708); both are the GPU-bound uncapped pacing described above
(p50 6.6 and 6.0 ms), and late pipelines are 0 in all six runs.

## The Rust client at WP 3.9 (Level 1's kinds and animators, DECISIONS D490 to D497)

2026-10-04, the same method (`node tools/parity/rust-perf.mjs --level
sierra`, headless Chrome, 1280 × 800, high quality, uncapped, the full
route), A/B against the build before WP 3.9's client (1bb2dd6), alternating
runs. The machine was shared with other agents' builds the whole time
(1-minute load average 8 to 21 at the starts, higher during runs), so single
slow frames come and go in both builds.

| Build, backend | Load | Frame ms p50 / p95 / p99 / max | > 50 ms (30 s / all) | Ready |
|---|---:|---|---|---:|
| before, WebGPU | 9.5 | 3.7 / 8.1 / 12.1 / 34.8 | 0 / 0 | 3.9 s |
| WP 3.9, WebGPU | 8.8 | 3.2 / 8.0 / 12.2 / 59.9 | 0 / 4 | 7.1 s |
| before, WebGPU | 8.9 | 4.6 / 12.6 / 24.0 / 69.1 | 0 / 4 | 4.6 s |
| WP 3.9, WebGPU | 21.3 | 3.3 / 8.6 / 12.0 / 243.4 | 0 / 2 | 8.3 s |
| before, WebGPU | 18.3 | 3.8 / 12.4 / 23.7 / 126.2 | 1 / 14 | 3.2 s |
| WP 3.9, WebGPU | 21.1 | 3.8 / 12.0 / 19.4 / 123.2 | 0 / 4 | 10.9 s |
| before, WebGL2 (30 s) | | 1.6 / 19.1 / 20.0 / 36.6 | 0 / 0 | |
| WP 3.9, WebGL2 (30 s) | 9.3 | 2.6 / 19.2 / 20.7 / 37.2 | 0 / 0 | |
| WP 3.9, WebGL2 (30 s), `?world=off` | 10.8 | 2.3 / 35.7 / 38.3 / 55.2 | 34 / 34 | |

No pipeline was compiled after the warm-up in any run (61 combinations
now, 52 before: the new kinds). The frame-time distribution is the same;
the slow frames are isolated, at different places from run to run, in
both builds. On WebGL2 the uncapped page's GPU-bound pacing (frames of
17.7, 36 or 53 ms, described above for Coast) comes and goes between
identical runs (the `world=off` row, and a full-route WebGL2 run of this
build with 231 such frames in its first 30 s next to one with none).

The animator path itself, timed in the wasm build over the full flight
(every 300 frames): 0.04 to 0.15 ms a frame on average, at most 3.2 ms.
It allocates no GPU object per frame: material values go to the globals
(D490), moved instances are written into their stream's buffer in place
(D497), and the flag's 18 vertices are rewritten in Bevy's mesh slab. The
world build is done before `ready` (2.9 s in wasm on the dev machine).

Memory, ten reloads of Sierra (WebGPU): after load 348 MB, then 357, 488
(before: 303, then 443). With `?world=gen`: 348, then 357, 375.

Size: `mp_game_bg.wasm` 9.44 MB and `mp_game_webgl2_bg.wasm` 9.93 MB after
gzip (budget 10 MB; before: 8.77 and 9.24). World generation is now linked
into the client (the level build, its textures and the bundled fonts).
After merging main (race audio, the race warm-up, the other levels'
world generation) and building Level 1's scenery by name (D498): 9.60 and
10.08 MB, the WebGL2 build 0.08 MB over the budget; the budget question is
the owner's (D498).

**Races under vsync, and the WebGL2 Sierra question (2026-10-04).** The
uncapped WebGL2 Sierra race above (12 frames over 50 ms) was run on a
build of 5350be2, before the WP 3.9 client animators and the race audio
were merged, so those could not have caused it. Run as browsers run it
(vsync on, `CAPPED=1`, 30 s races, `timescale=2`), four builds alternating:
1bb2dd6 (before the animators, and before the car-model warm-up of D458),
ba1f5f1 (with the animators), d2550fe (with the race audio) and ecae56e
(the 24-float instance stream), on Sierra and Coast, WebGL2 and WebGPU, two
rounds (load 7 to 32, other GPU users at 0 to 61 % between runs):

- Frames over 50 ms under vsync: 0 to 1 per run in every build and on both
  backends, except one 850 ms stall (d2550fe, Coast, WebGL2, s 1,867, at
  load 7; not seen again).
- Frames over 33 ms on WebGL2: 0 to 16 on Sierra and 0 to 36 on Coast,
  with no order by build (1bb2dd6: 0, 0, 4, 1; ba1f5f1: 0, 7, 0, 11;
  d2550fe: 11, 0, 33, 36; ecae56e: 16, 0, 26, 4). WebGPU: 0 to 6 in every
  build.
- `lateFrames` 1 and 3 on 1bb2dd6 and ba1f5f1 (no D458), 0 from d2550fe on.

To tell the client's work from the machine, a second alternating series
timed every `requestAnimationFrame` callback (the client's frame work, on
WebGL2 where Bevy batches on the CPU). In the quiet runs (Coast at load
3.8 to 4.8, all four builds; Sierra at 8.0 to 13.3 for ba1f5f1, d2550fe
and ecae56e) the callback is 3.9 to 5.7 ms at the median and at most 7.4
to 20.9 ms, and no frame misses a vsync. Long callbacks come in whole runs
and in every build, the oldest included: 1bb2dd6 had 60 and 79 over 16.7
ms on Sierra and 64 on Coast, ba1f5f1 159 on Sierra, d2550fe and ecae56e
778 and 755 on Sierra (median 17 ms, load 13.5 and 29.7), and those are
the runs with frames over 33 ms. d2550fe and ecae56e were clean on Sierra
in the next round, so it is not a commit. The load average does not catch
every busy spell (ba1f5f1's bad run started at 7.3), but the slow frames
on WebGL2 follow the machine, not the code. The one
difference between builds at low load is the median callback on Coast:
3.9 and 4.0 ms (1bb2dd6, ba1f5f1), 4.5 (d2550fe), 5.0 (ecae56e). That is
about 1 ms of frame work added with the audio and the new kinds, well
inside the 16.7 ms.

## Wasm size: where the bytes go (WP 9.1 brought forward, DECISIONS D670 to D674)

2026-10-04. Sizes are MB of 2^20 bytes, as `cargo xtask size` prints them;
"gzip" is gzip at level 9 (`xtask size` uses flate2's best, which comes
out about 0.01 MB above `gzip -9`). Main at a60cb88 was over SPEC 6.6's
budget: `mp_game_bg.wasm` 31.27 MB, **10.00 MB gzip** (WebGPU);
`mp_game_webgl2_bg.wasm` 32.51 MB, **10.48 MB gzip** (WebGL2).

**How it was measured.** The release build's wasm (`target/wasm32-
unknown-unknown/web-release/mp_game.wasm`, which keeps its name section)
through `wasm-bindgen` and then `wasm-opt -Oz -g`, the same optimisation
as the shipped file but with function names kept (32.79 MB without the
names, as shipped). Each function body is given to a crate: the first
crate in its demangled name that is not std, core or alloc (an impl's
self type comes first, so generic std code goes to the crate it was
instantiated for), and a Bevy system (`FunctionSystem<..., f>`) to the
crate of `f`. Data is split by content: the bundled font files found
byte for byte, WGSL sources, other text, the rest. The gzip column
compresses each part on its own and scales the parts to the file's
gzip size, so it is an estimate; generic code instantiated inside
`bevy_ecs` for another crate's types stays in the first row, so the rows
of the crates around it are undercounts (removing the 2D sprites saved
0.25 MB where their own row says 0.15). The linker map (`-C link-arg=
--Map`) does not help: with fat LTO all data is anonymous. `twiggy top`
on the same named file gives the per-function sizes.

| Part (WebGPU, a60cb88) | Raw MB | Gzip MB (est.) | Share |
|---|---:|---:|---:|
| Bevy ECS, app, reflect, asset, math, tasks, log (with every system's generic code, `erased_serde`, `ron`, `glam`, `indexmap`) | 10.31 | 2.53 | 25 % |
| Data: binary (vtables, Unicode and ICU tables, regex tables, constants) | 2.75 | 1.00 | 10 % |
| Bevy UI and text (`bevy_ui`, `ui_render`, `bevy_text`, taffy, parley, swash, zeno, harfrust 0.6, skrifa 0.42 and 0.44, read-fonts 0.39 and 0.41) | 2.66 | 0.96 | 10 % |
| naga, naga_oil, regex (WGSL composition and validation on the client; includes naga's GLSL front end, which `bevy_shader` turns on for wasm) | 2.16 | 0.83 | 8 % |
| Bevy PBR, core pipelines (3D and 2D), lights | 2.23 | 0.76 | 8 % |
| Data: the bundled fonts (`mp_canvas`, 17 files, `include_bytes!`) | 1.07 | 0.65 | 7 % |
| Bevy render core, mesh, image, camera, `image` and `png` | 1.94 | 0.64 | 6 % |
| `mp_worldgen`: the other levels' scenery (Coast, Beach, Harbor, Desert, Raceway, Streets) | 1.19 | 0.39 | 4 % |
| `mp_canvas`'s text stack (harfrust 0.13, skrifa 0.46, read-fonts 0.43) | 0.82 | 0.30 | 3 % |
| Bevy input, window, winit, a11y | 1.03 | 0.29 | 3 % |
| `mp_game` | 0.62 | 0.24 | 2 % |
| `mp_worldgen`: shared (terrain, road, sky, sea, flora, textures, car models, geometry) | 0.58 | 0.23 | 2 % |
| `mp_worldgen`: Sierra's scenery (Mountain, Valley, City) | 0.51 | 0.18 | 2 % |
| Bevy 2D sprites (`bevy_sprite`, `bevy_sprite_render`) | 0.49 | 0.15 | 2 % |
| Data: WGSL sources (Bevy's and the client's) | 0.61 | 0.15 | 2 % |
| Data: other text (type names, messages, JSON) | 0.44 | 0.12 | 1 % |
| std, core, alloc | 0.31 | 0.12 | 1 % |
| Simulation (`mp_sim`, `mp_levels`, `mp_track`, `mp_math`, `mp_net`) | 0.26 | 0.11 | 1 % |
| `mp_canvas` and tiny-skia | 0.23 | 0.10 | 1 % |
| `mp_audio` | 0.18 | 0.07 | 1 % |
| `mp_scene`, serde_json, sha2 | 0.19 | 0.07 | 1 % |
| Bevy `post_process` (bloom, depth of field, motion blur, ...) | 0.16 | 0.06 | 1 % |
| wgpu and the web-sys glue | 0.09 | 0.04 | 0 % |
| Section headers, import and export names, the function table | 0.54 | | |
| **Whole file** | 31.27 | 10.00 | |

The WebGL2 file is 0.48 MB larger after gzip: wgpu-core, wgpu-hal's GLES
backend, glow and naga's GLSL back end, which the browser's WebGPU
replaces in the other build. wgpu itself is small on WebGPU.

**Duplicate crates** (`cargo tree -d`, wasm target): two font stacks
(above; three copies of read-fonts and skrifa, two of harfrust,
font-types), codespan-reporting 0.12 and 0.13 (naga_oil and naga),
hashbrown 0.16 and 0.17 (bevy_platform and indexmap), miniz_oxide 0.8 and
0.9 (both from `png`: its decoder and flate2), syn 2 and 3 (proc macros,
not in the wasm). Only the font stacks are ours to change: `mp_canvas`'s
copy is what world generation draws its signs with, reached from the
client's world build (D491), not from the menus (their icons draw paths
only).

**Trims committed** (no behaviour change; checked with Seaside's 29
stations on both backends and Sierra's 06250 to 07500 through the web
build, pixel for pixel against the build before, the phone harness, the
workspace tests and `cargo xtask parity materials`):

| Change | WebGPU gzip | WebGL2 gzip |
|---|---:|---:|
| a60cb88 | 10.00 | 10.48 |
| No `bevy_post_process` (D670) | 9.85 | 10.33 |
| Debug and trace logging compiled out of release wasm (D671) | 9.81 | 10.28 |
| `ClientPlugins`: `DefaultPlugins` less the 2D sprites (D672) | 9.56 | 10.01 |
| Main at c609122 (menus +0.14, WP 3.9's Sierra-only scenery factory -0.40), without the trims | 9.73 | 10.22 |
| **c609122 merged with the trims (this branch)** | **9.29** | **9.76** |

The pictures through the merged build equal c609122's (Seaside on both
backends and Sierra's stretch; WebGL2's usual one or two pixels apart),
and the e2e suites (`menu`, `race-flow`, `level-switch`, `race-button`:
30 tests), the phone harness (`tools/parity/e2e/phone.cjs`, touch and
autopilot) and `phone-menu.mjs` pass on it.

**Build settings, measured but not changed** (on a60cb88 plus the three
trims; gzip -9 MB; frame times from `tools/parity/rust-perf.mjs`, 40 s
flights uncapped at 1280 × 800, high quality, three alternating rounds at
load average 1.5 to 3.3 (the wasm-opt rows two rounds, load up to 14.5);
"p50" is the median frame in ms, "ready" the seconds from navigation to
ready, which on Sierra includes the client's world build in wasm, the
most CPU-bound number here):

| Setting | WebGPU gzip | WebGL2 gzip | Sierra WebGPU p50 / p95, ready | Sierra WebGL2 p50 / p95, ready | Seaside WebGPU p50 / p95 | Seaside WebGL2 p50 / p95 | Pictures |
|---|---:|---:|---|---|---|---|---|
| Current: `opt-level = 3`, `wasm-opt -Oz` | 9.54 | 10.00 | 2.1 / 9.2, 5.5 s | 1.7 / 19.0, 6.2 s | 2.9 / 6.4 | 0.9 / 7.1 | |
| `opt-level = "s"` | 8.22 | 8.60 | 2.3 / 9.2, 6.1 s | 1.8 / 19.4, 6.7 s | 2.7 / 6.4 | 1.1 / 4.5 | identical |
| `opt-level = "z"` | 7.18 | 7.51 | 2.7 / 8.3, 7.0 s | 2.0 / 19.7, 7.7 s | 2.3 / 5.6 | 1.3 / 4.4 | 5 single pixels differ on WebGPU (up to 40 levels), reproducibly |
| `wasm-opt -O3` | 9.68 | 10.14 | 2.1 / 9.3, 5.5 s | 1.7 / 19.0, 6.3 s | 2.8 / 6.6 | 0.9 / 9.2 | |
| `wasm-opt -O4` | 9.71 | 10.18 | 2.1 / 8.8, 5.4 s | 1.7 / 19.0, 6.3 s | 2.8 / 6.3 | 0.9 / 8.7 | |
| `wasm-opt -Oz --converge` | 9.54 (-4 KB) | 10.00 (-5 KB) | not run (the same code within 5 KB) | | | | |
| `strip = true` | no change | no change | | | | | |
| `-C target-feature=+simd128` | 9.39 | 9.84 | 2.1 / 9.2, 5.4 s | 1.7 / 19.0, 6.2 s | 2.8 / 6.5 | 0.9 / 7.5 | identical |

- `opt-level` "s" and "z" cost CPU time: Sierra is ready 0.5 to 0.6 s
  (8 to 11 %) later with "s" and 1.5 s (24 to 27 %) later with "z",
  most of it the world build in the page, and the CPU-bound median
  frames grow by 0.1 to 0.2 ms ("s") and 0.3 to 0.6 ms ("z") on Sierra and
  on Seaside's WebGL2. Seaside on WebGPU and the 95th percentiles move the
  other way or not at all, because uncapped those frames wait for the GPU.
  On a phone, whose CPU is several times slower, the CPU share is what
  grows. Not decided (the owner's call, D674).
- `wasm-opt -O3` and `-O4` are 0.14 to 0.18 MB larger and no faster
  measurably; `--converge` saves 4 to 5 KB for six times the wasm-opt time
  (46 s to 273 s). `-Oz` stays.
- `strip`: wasm-opt already drops the name section; the shipped file's
  custom sections are `producers` and `target_features` (219 bytes).
- `simd128` lets LLVM move 16-byte values with one load and store: 0.16
  MB smaller, no frame-time change, pictures identical, and the
  simulation's tests in wasm pass with it (release, Node). Every browser
  with WebGPU has wasm SIMD (Chrome 91, Firefox 89, Safari 16.4); the
  WebGL2 fallback would lose Safari before 16.4. Not committed (D674).
- The simulation's tests in wasm pass at `opt-level` "s" and "z" too (21
  test binaries each, release profile): the floating-point results do not
  depend on the optimisation level.

**Compression** (the merged build; how files are delivered is the
owner's, D439 and D674):

| Encoding of the merged build | WebGPU | WebGL2 |
|---|---:|---:|
| gzip -9 (as now) | 9.27 | 9.73 |
| gzip by zopfli (`pigz -11`; any browser decodes it, only the build's `precompress` changes) | 8.86 | 9.31 |
| brotli 11 (`Content-Encoding: br`; browsers send `Accept-Encoding: br` only over https, which the tailnet front and Pages are) | 5.82 | 6.12 |

**Frame time of the merged build against c609122** (two alternating
rounds of all four, then four more of Seaside on WebGL2; load 1.9 to 3.1):
the same on Sierra (both backends) and Seaside on WebGPU (p50 2.0 to 2.1,
1.8, 2.8 ms; ready within 0.2 s). Seaside on WebGL2, uncapped, is the one
difference: mean 1.7 against 1.5 ms and p95 10.5 against 7.4 ms, every
run, with the median frame 0.9 against 1.0 ms. Bisected to D672 (a60cb88
with D670 and D671: p95 6.4 to 7.2; with D672: 10.2 to 10.5). With vsync
(`--capped`) the two are the same (16.7 ms at the 50th, 95th and 99th
percentiles, worst 17 ms, both builds). Uncapped at about 600 frames a
second this looks like the GPU-bound pacing of WebGL2 described above (a
page with a little less work per frame, waiting longer on some), not
work added; with vsync on, as players run it, nothing changes.

**Each further level's world generation, if linked** (a standalone
cdylib of `mp_worldgen` with the stages and a chosen set of scenery
modules, fat LTO, `wasm-opt -Oz`, gzip -9; deltas over the Sierra set):

| Added to Sierra's Mountain, Valley and City | Gzip |
|---|---:|
| Seaside Raceway (`Raceway`) | +0.03 MB (34 KB) |
| Coast Highway (`Coast`, `Beach`, `Harbor`) | +0.20 MB (200 KB) |
| Desert Run (`Desert`) | +0.14 MB (140 KB) |
| Downtown Streets (`Streets`) | +0.10 MB (106 KB) |
| Night City Cruise (`City` only, already in) | 0 |
| All of them | +0.44 MB (452 KB) |

Alone, without Sierra's, each costs far more (Raceway 0.47 MB, Coast's
three 0.74, Desert 0.67, Streets 0.63; Sierra's three 0.75 over the
stages), because the builders, flora, textures, geometry and the text
stack they share come in with the first module. The client-side animator
and kind code each level brings (`animate`, `render`) is extra and not
counted here. The stages and the fonts alone are 0.84 MB. (KB here are 1024 bytes.)

## Coast Highway built in the client (DECISIONS D700 to D703)

2026-10-04, the web release build (`opt-level = "s"`), headless Chrome on
WebGPU, 1280 × 800, high quality, through the registered server
(`rust-perf.mjs`, a race script reading `__mp.wasmMemoryBytes`). GPU memory
is the tab's processes' use as `nvidia-smi` lists it, sampled every 2 s.
The machine was overloaded throughout (load averages given per run, up to
190 on 24 cores, swap full, other agents' browsers on the same GPU), so
frame times are indications.

| Run | Load | Frame ms p50 / p95 / p99 / max | Wasm MB after load → reloads | VRAM max |
|---|---:|---|---|---:|
| Rust `?world=gen`, full route | 21 → 46 | 5.9 / 28.6 / 46.7 / 282.8 | 488 | |
| Rust, export, animators off (`?world=off`), full route | 36 → 58 | 4.2 / 8.5 / 15.7 / 219.3 | 440 | |
| JS game, full route | 47 → 36 | 1.2 / 6.0 / 15.7 / 242.3 | | |
| Rust `?world=gen`, 20 s and ten reloads | 40 | 4.8 / 19.1 / 33.4 / 92.7 | 456 → 512 (all ten) | 2.33 GB |
| Rust, export with animators, 20 s and ten reloads | 64 | 5.5 / 22.7 / 39.1 / 153 | 456 → 663 (all ten) | 1.58 GB |
| Rust `?world=gen`, race to results (autopilot, timescale 4) | ~100 | | 466 | 1.36 GB |
| Rust, export, race to results | ~100 | | 466 | 1.38 GB |
| Rust `?world=gen`, flight, before D701's fix | 15 | 11.6 / 17.2 / 17.3 / 55.5 | 504 | 6.1 GB, growing |
| Rust `?world=gen`, 40 s flight, after the fix | 49 | | | 1.86 GB |
| Rust `?world=off`, 30 s flight | 8.5 | | | 1.66 GB |
| Sierra `?world=gen`, 40 s flight (VRAM from a second run at 9) | 3.5 | 2.7 / 6.9 / 8.8 / 53.8 | 347 | 1.57 GB |
| Native release, Coast `world=gen`, 6,000 frames | n/r | | | 0.83 GB flat |

Alternating 60 s flights with the animators on and off (`?world=gen`
against `?world=off`, and the export with and without them) at load 100
to 190 did not separate them: medians within 0 to 1.4 ms either way, p95
from 8 to 49 ms in both. Natively, timed in the client over Coast's
route, the animator path is 0.10 ms a frame (the updaters 0.024 ms; 124
edits a frame: 64 instance colours, 26 instance matrices, 7 transforms, 17
numbers, 4 colours, 3 texture offsets, 2 `Points` attributes). The world
build: 1.2 s natively, 3.1 to 3.3 s in the web build. Ready from
navigation 6.5 to 8.4 s with `?world=gen` (nothing downloaded), 8.1 to 9.4
s with the export (199.8 MB downloaded locally in 3.2 to 3.8 s).

## Frame time against the JS game (DECISIONS D860 to D867)

2026-10-04. SPEC 6.6: frame time no worse than the JS game on the same
device and settings. Both games measured the same way by
`tools/parity/rust-perf.mjs` (D860): the fly camera at 60 m/s from s = 80
(the Rust flights with `cars=0`, as the JS fly camera has no cars) or a
race with the autopilot (`--race`), 1280 × 800, high quality, headless
Chrome on the dev machine's RTX 3060, WebGPU unless marked; the JS game,
the Rust build before (main at 2718502) and after alternating level by
level. Per frame, in ms:

- **main**: the renderer main thread's CPU time (Chrome trace, thread
  time, so time descheduled by other load is left out);
- **GPU proc**: the GPU process main thread's CPU time (where Chrome runs
  the page's WebGL or WebGPU commands);
- **rAF**: the median time inside the page's `requestAnimationFrame`
  callbacks (wall time: includes waiting on the GPU process);
- **fps**: median over half-second windows (uncapped runs only).

"Load" is the 1-minute load average, min to max over the runs (24 cores;
other agents' work). The GPU was shared too: other users had it at 100 %
utilisation before the uncapped runs began. "After" for the 60 Hz,
phone, WebGL2 and race rows is the final build (fix4, D861 to D865); the
uncapped rows were taken with D861 to D864 (the HUD change, D865, does
not run without a race).

**Desktop, WebGPU, 60 Hz** (vsync, as browsers run it; one round, load 6
to 16):

| Level | JS main | before | after | JS GPU proc | before | after | JS rAF | before | after |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Sierra | 1.12 | 1.10 | **0.90** | 0.85 | 1.69 | 1.29 | 2.3 | 3.1 | 2.6 |
| Coast | 1.53 | 1.40 | **0.96** | 1.06 | 1.67 | 1.11 | 2.8 | 3.9 | 2.6 |
| Streets | 0.72 | 0.90 | 0.78 | 0.62 | 1.37 | 1.03 | 1.2 | 2.6 | 2.1 |
| Desert | 0.78 | 1.09 | 0.93 | 0.60 | 1.45 | 1.12 | 1.3 | 2.8 | 2.3 |
| Seaside | 0.36 | 0.90 | 0.78 | 0.36 | 1.60 | 1.24 | 0.5 | 2.4 | 2.0 |
| Cruise | 0.69 | 0.87 | 0.79 | 0.67 | 1.36 | 1.02 | 1.1 | 2.4 | 2.2 |

**Desktop, WebGPU, uncapped** (BASELINE's earlier convention; two rounds,
medians; load 8 to 128, so read the CPU columns):

| Level | Load | JS main | before | after | JS GPU proc | before | after | JS fps | before | after |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Sierra | 20–128 | 0.89 | 1.10 | **0.83** | 0.56 | 1.98 | 2.10 | 120 | 101 | 105 |
| Coast | 30–91 | 1.17 | 1.58 | 1.42 | 0.68 | 2.63 | 2.67 | 109 | 109 | 102 |
| Streets | 23–79 | 0.45 | 1.47 | 1.10 | 0.28 | 2.10 | 1.90 | 171 | 99 | 148 |
| Desert | 14–43 | 0.93 | 1.31 | 1.19 | 0.66 | 3.06 | 2.71 | 115 | 100 | 100 |
| Seaside | 10–34 | 0.22 | 1.01 | 0.95 | 0.18 | 1.94 | 1.50 | 200 | 154 | 154 |
| Cruise | 8–24 | 0.38 | 1.01 | 0.80 | 0.24 | 2.02 | 1.85 | 172 | 140 | 168 |

Uncapped, the Rust client's GPU-process thread is busy 97 to 100 % of
the time in every run (the JS game's 60 to 90 %): that thread, not the
page, sets its pace (D866 item 2).

**A race at 60 Hz** (the sports car on the autopilot, seed 1, 30 s from
race time 3 s; two rounds, load 7 to 12):

| Level | JS main | before | after | JS GPU proc | before | after | JS rAF | before | after |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Sierra | 2.34 | 2.69 | **2.25** | 1.48 | 3.49 | 1.88 | 3.2 | 4.9 | 4.1 |
| Coast | 2.00 | 2.88 | 2.23 | 1.26 | 3.58 | 1.97 | 3.4 | 5.0 | 4.3 |

**Phone emulation** (844 × 390 at dpr 3, touch, high quality off, CPU
throttled 4 ×, 60 Hz; one run each, load 7 to 16). The rAF column is the
page's share of the 16.7 ms frame:

| Level | JS main | before | after | JS rAF | before | after |
|---|---:|---:|---:|---:|---:|---:|
| Sierra | 3.09 | 5.31 | 3.60 | 5.4 | 13.5 | 10.0 |
| Coast | 5.82 | 3.85 | 4.51 | 9.5 | 11.5 | 12.2 |
| Seaside | 2.22 | 4.17 | 3.45 | 3.0 | 11.2 | 9.3 |

**WebGL2** (uncapped, one run each, load 8 to 12):

| Level | JS main | before | after | JS fps | before | after |
|---|---:|---:|---:|---:|---:|---:|
| Sierra | 0.85 | 1.50 | 1.43 | 109 | 28 | 32 |
| Coast | 0.82 | 1.42 | 1.28 | 102 | 25 | 28 |
| Desert | 0.68 | 1.16 | 1.10 | 110 | 38 | 37 |

What each fix saved (A/B on its own, Sierra unless said): no light
clustering (D861) main thread 1.71 to 1.42 ms, GPU process 2.62 to 2.10
(load 40); `render_system` without the empty submission (D862) GPU
process 2.71 to 2.15 ms (three pairs, load 20 to 50); the level ids kept
(D863) about 0.17 ms of wall time a frame in the profile; the gamepad
bridge written on change (D864) about 0.09 ms of a race frame's wall
time; the HUD's uniforms written in place (D865) about 0.12 ms of a race
frame's wall time and the GPU objects made per frame. The race's GPU
process went from 3.5 to 1.9 ms a frame with all of them.

**Pictures.** The build after against the build before, each in turn as
`dist/next`, through the same tools: every level's screenshot stations
from the default (built) path on WebGPU (`rust-web-stations.mjs`, 398
stations), 392 identical to the pixel and six with one pixel different
(Coast 04250-high, five of Seaside's high stations); flying those again
from both builds, the same single pixels flip between two runs of either
build, so they are run-to-run noise, not the change. Against the JS
shots (`cargo xtask parity shots`): 398 stations, 0 over the limits,
worst 0.925 mean ΔE00 and 5.171 block 95 %, as before. WebGL2, three
stations a level (18): all identical. The five staged effect scenes
(`effects-scenes.mjs`) on WebGPU and WebGL2: all identical. The HUD
(`hud-shots.mjs`: Sierra at 20 s desktop and iPhone portrait, Sierra at
101.5 s, Seaside at 40 s, the Cruise at 30 s, two runs of each build):
the dial, minimap, speed lines and texts draw the same; the scenes behind
differ between any two runs by the race's frame timing, as D826 notes.
The `gamepad` (8 tests) and `race-flow` (11) e2e suites pass on the
final build.

**`simd128`** (D867, the owner's call): two alternating rounds at 60 Hz,
load 6 to 7, main thread 0.88 / 0.87 ms against 0.99 / 0.88 (Sierra), 0.75
/ 0.76 against 0.82 / 0.78 (Seaside); 8.83 against 8.99 MB after gzip.

**How to run.** `cargo xtask web --release`, then (registered server up)
`node tools/parity/rust-perf.mjs --level sierra --reloads 0 --secs 40
--trace 10 --query cars=0 [--capped] [--race] [--phone --throttle 4 --hq
0] [--backend webgl2] [--dist <dir under dist/>]` and the same with
`--game js`; the summary line carries `cpu` (rAF), `trace`
(`cpuPerFrameMs`, `gpuCpuPerFrameMs`, `gpuBusyShare`, `allCpuPerFrameMs`)
and `loadDuring`. The session's scripts (alternating runs, the profiler
over a names-kept wasm, WebGPU call counts, the GPU-process trace) are in
its scratchpad, not the repo.

## A framerate pass on the phone (DECISIONS D1180 to D1182)

2026-10-09, main aa815d8 against the same tree with D1180 to D1182,
alternating build by build, two rounds each. `rust-perf.mjs --race
--capped --phone --throttle 4 --hq 0 --trace 10` (the sports car on the
autopilot, 30 s from race time 3 s; 844 × 390 at dpr 3, CPU 4 × slower,
60 Hz), WebGPU, load 1.6 to 2.7. Per frame: the page's work (rAF, median
and 95th percentile), the renderer main thread's CPU time (main), and
frames over 33 ms in the 30 s.

| Level | rAF p50 before | after | rAF p95 before | after | main before | after | >33 ms before | after |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Seaside | 15.1, 14.5 | **9.1, 9.6** | 19.3, 18.5 | 13.3, 13.8 | 8.83, 8.47 | 5.36, 5.66 | 21, 6 | 0, 1 |
| Coast | 11.7, 11.5 | 10.6, 10.3 | 16.0, 16.1 | 15.4, 14.7 | 6.43, 6.35 | 5.95, 5.76 | 0, 0 | 1, 0 |
| Sierra | 10.9, 10.7 | 10.0, 9.6 | 14.9, 14.9 | 14.2, 14.3 | 6.02, 5.84 | 5.56, 5.37 | 0, 0 | 0, 1 |

Seaside before with `?life=0` (no people or birds): 9.5 and 9.3 ms, 0
frames over 33 ms. The desktop at 60 Hz was not the problem (Sierra's
race: main 1.69 ms a frame, GPU process 1.64, against 2.25 and 1.88 at
D866); uncapped, Coast and Sierra raced at about 390 fps and Seaside at
286 before.

What each frame spent before, from a CPU profile of a names-kept build
(wasm-bindgen `--keep-debug`, wasm-opt `-g`) attributed to Bevy systems:
on Seaside `run_animators` 30 % (the people, D1180); on Coast and Sierra
the opaque pass 18 to 20 %, of which `map_wgt_limits` 7 % (D1182); the
`__mp` bridge about 3.5 % everywhere (D1181). Left as they are: Bevy UI's
layout and text measuring (about 4 %), the audio graph's Web Audio calls
(about 2 %), and the per-draw cost of the opaque pass itself (D866 item
2). WebGL2, Coast's race uncapped on the desktop: 84 fps (D866: 28) with
a 99th percentile of 38 ms; neither the main thread nor the GPU process
is saturated, not looked into further.
