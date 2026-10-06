# Midnight Racer in Rust: roadmap

Companion to [SPEC.md](SPEC.md). The spec says what to build and how it is
judged; this says in what order, in what pieces, and what ends each stage.

## How to read this

- A **milestone** (M0 to M11) ends at a gate with a demo the owner can try.
- A **work package** (WP) is one agent session's worth of work, or a few. It
  names the JS it ports, the Rust it produces, and the parity layers (L1 to
  L5, SPEC section 12) that gate it.
- **Size** is the JS being ported: S under 1,000 lines, M 1,000 to 3,000,
  L 3,000 to 8,000, XL more.
- Packages listed as parallel have disjoint files. Shared modules follow the
  rule that worked in the JS project: one owner; everyone else may add
  exports but not change what exists.

```
M0 ──► M1 (simulation) ───────────────────────────┐
  │                                               ▼
  └──► M2 (render gate G1) ──► M3 (world, Level 1) ──► M4 (playable) ──► M6 (UI, input)
                 │                                                          │
                 └──► M5 (audio; any time after M2) ────────────────────────┤
                                                                            ▼
                         M7 (other levels, in parallel) ──► M8 (pursuit) ──► M9 (cutover)
                                                                            │
                                                             M10 (multiplayer) ──► M11
```

M1 and M2 run side by side. M5 needs only the client shell from M2. The RL
environment (M12c) needs only M1 and can start whenever wanted.

## Working rules for every package

1. Read the JS files named in the package in full, and the matching section
   of the inventory, before writing Rust.
2. Port faithfully: same structure, names, constants, order of operations
   and order of random draws. Carry over the comments that explain why.
3. Write the parity test first where a golden exists, then port until it
   passes.
4. A behaviour change is a deviation: add it to `DEVIATIONS.md` with the
   reason. A choice the spec does not cover: add it to `DECISIONS.md` and
   carry on with the option that best preserves parity.
5. Do not edit the JS game except to fix a bug the owner reports, or to add
   a parity hook during M0 (SPEC 12). A hook changes nothing unless switched
   on. After the `js-reference` tag at the end of M0, hooks are closed too.
6. Check your own work the way the JS project does: run the tests, render
   it headless, look at the result, compare with the reference. Then commit
   and push to `main`.
7. Local output is viewed through the registered server only (`proj up
   midnight-racer`). Never start another server or write a port number
   anywhere.

---

## M0. Foundations and the oracle

**Goal:** a Rust workspace that builds for native and web, and tooling that
turns the JS game into reference data.

| WP | Work | Output | Gate |
|---|---|---|---|
| 0.1 | Workspace, empty crates with the dependency rules, `xtask` (`check-deps`, `web`, `size`), `CLAUDE.md`, `DEVIATIONS.md`, `DECISIONS.md`. The web output is a bare wasm-bindgen module that writes a line to the page: it proves the pipeline without choosing an engine version | Cargo workspace at the repo root; `cargo xtask web` produces `dist/next/` | Builds native and wasm; `check-deps` passes; the page opens on a phone at the tailnet address |
| 0.2 | CI: format, clippy, tests, wasm tests, `check-deps`, web build, size report, JS unit tests. No deploy: GitHub Pages stays the JS game until M9 (since D1101 a separate Pages workflow also publishes the Rust build at `dist/next/`) | `.github/workflows/` | Green on `main`; the live site is unchanged |
| 0.3 | The math kernel and the first hooks. `mp_math`'s kernel on `libm` (every inexact `Math` function, with n-argument `hypot`; SPEC 4.2), built to wasm for the oracle. JS hooks: replace those `Math` functions with the kernel and make any other inexact one throw; rewrite `**` as `Math.pow` in simulation and world-generation code; optional generator parameters in `AIDriver`, and in `PursuitView` and `Pursuit` through to `PoliceDriver`; fixed dt with several ticks per frame; quantised inputs | `crates/mp_math` (kernel only), `tools/parity/kernel/`, hooks in `src/` | JS suites pass with hooks off and with the kernel on; kernel in wasm and native give identical bits on a million inputs per function |
| 0.4 | Simulation references, all taken with the kernel on. The trace record format and the recorder (SPEC 4.6), which fails on any page error; the scenario catalogue; module traces from Node; whole-race recordings from headless Chrome for every level, in race mode and Hot Pursuit; the fuzz excursion and ticks-per-second baselines of the JS. Dumps the simulation needs from the JS world: `runout`, the opposite carriageway's range and lanes, vehicle dimensions for all kinds | `parity/trace-format.md`, `parity/scenarios.md`, `tools/parity/sim-*.mjs`, `parity/golden/sim/` | Two runs of every recording give identical files |
| 0.5 | Scene export, taken with the kernel on. `mp_scene` types and the `.mrscene` reader and writer; `MaterialKind` tags on every JS material; the exporter (walks `__world.root` and the car models); Track and terrain dumps. An option exports terrain, road and sky only | `crates/mp_scene`, `tools/parity/scene-export.mjs` | Every mesh has a kind; the file read back by `mp_scene` has the same per-mesh digests (counts, bounds, texture hashes) as the live scene; all six levels export |
| 0.6 | Pictures. Screenshot stations with traffic and particles frozen; material test scenes built from the exposed JS patch functions; the comparison metric; the report page | `tools/parity/shots.mjs`, `cargo xtask parity shots`, `parity/report/` | JS against JS gives the noise floor; thresholds in SPEC 12 confirmed or raised |
| 0.7 | Audio reference: engine wave arrays, kit and SFX buffers, offline renders, a call log for a scripted drive | `parity/golden/audio/` | Reproducible. Then tag `js-reference` |
| 0.8 | Baselines on the reference phones: the JS game in a race and in the fly camera along each route (`?s=&v=`): frame rate, load time, memory | `docs/rust-port/BASELINE.md` | Owner reads the phone numbers |

**Order:** 0.1 first. Then 0.2, 0.3 and 0.5 together. 0.4 needs 0.3. 0.6
and 0.7 need 0.5's hooks pattern but not its output.

**Owner:** name the reference phones (the iPhone must be on iOS 26 or later;
an Android 12+ phone if one is to be supported) and join them to the
tailnet; read the stats overlay for 0.8.

**Exit:** goldens exist and are reproducible; the report page shows the JS
compared with itself; CI is green; `js-reference` is tagged.

---

## M1. Simulation at parity (headless)

**Goal:** the whole game's logic in Rust with no graphics, matching the JS
reference tick for tick. Size L (about 3,300 lines of JS plus about 1,500
lines of tests).

| WP | Ports | Output | Gate |
|---|---|---|---|
| 1.1 | `util/math.js`; the JS-semantics helpers (SPEC 4.2) | `mp_math` complete | L1 (`math.test.js`); `mulberry32`, `hash2` and noise bit-exact against the JS run with the kernel |
| 1.2 | `track/Track.js`, `roadTypes.js`, level files, Seaside loader; `tools/seaside/build.py` gains a binary output; the world data and dimension tables from WP 0.4 | `mp_track`, `mp_levels`, `mp_sim::dims`, `assets/seaside/` | L1 (`track`, `levels`, `seaside` tests); every Track array identical to the JS dump |
| 1.3 | `CarPhysics.js`, `Vehicle.js` state | `mp_sim::{physics, vehicle}` | L1 (`physics.test.js`); module traces identical. **This is the proof that bit-identical works: do it before 1.4 to 1.6 and stop if it cannot be made to pass** |
| 1.4 | `Kinematic.js`, `AIDriver.js`, `Traffic.js`, `Collisions.js` | `mp_sim::{kinematic, ai, traffic, collisions}` | L1 (`ai`, `traffic` tests); module traces identical |
| 1.5 | Race rules from `Race.js`; the autopilot from `main.js` | `mp_sim::race`, `SimState`, `step`, `SimEvent`, `hash`, `trace_record` (begun in 1.3 for the player's fields) | Whole-race recordings identical for every level in race mode, and the cruise run |
| 1.6 | `Pursuit.js`, `PoliceDriver.js`, game rules from `PursuitView.js` | `mp_sim::pursuit` | L1 (`pursuit.test.js`); module traces and whole Hot Pursuit races identical |
| 1.7 | Determinism, snapshot, fuzz and speed tests (SPEC 4.6) | Tests and a benchmark | Native and wasm hashes equal; fuzz within the JS excursion; not slower than the JS |
| 1.8 | `mp-sim` command-line runner: run a race with the autopilot, print results, dump a trace or a state at a tick | A binary in `mp_sim` | Used by the gates above |

**Order:** 1.1, then 1.2, then 1.3 alone. Then 1.4, then 1.5, then 1.6.
1.7 and 1.8 grow alongside.

**Exit:** every ported test passes; every trace and recording is identical;
all six levels run headless and deterministically.

---

## M2. Render gate (G1)

**Goal:** find out, before the bulk of the work, whether Bevy through WebGPU
carries this game on the phones. Runs beside M1.

| WP | Work | Gate |
|---|---|---|
| 2.1 | Client shell: Bevy app, window, states, the HTML shell with the WebGPU check and loading bar, the build pipeline, the gesture bridge stub. Choose the Bevy version (SPEC 2) and record it. `tools/serve.py` sends precompressed files from `dist/` | Loads on desktop browsers, native, and the reference phones over the tailnet https address |
| 2.2 | Scene loader: `.mrscene` to Bevy meshes, instances and textures, with CPU copies dropped. A fly camera on the same parameters as the JS one | All six exports load |
| 2.3 | `three_std` shading library, the plain material kinds, the environment map from the sky, the post chain (bloom, tone mapping), fog, the shadow map, the sky dome | L4 material test scenes within threshold, on desktop |
| 2.4 | Terrain, asphalt, markings and sea kinds. Every kind not yet written draws with the plain standard material, marked as a stand-in; points as quads; thirty moving stand-in cars | The terrain-road-sky export of Sierra within threshold at five stations, on desktop |
| 2.5 | Screenshots of the Rust build: WebGPU flags and the wasm MIME type in the harness, a minimal `window.__mp.ready`, native `--screenshot` | The gates of 2.3 and 2.4 run from one command |
| 2.6 | Pipeline warm-up; frame-time, memory and size measurement; ten reloads | Numbers recorded in `BASELINE.md` |
| 2.7 | A WebGL2 build of the same client, selected by the page when WebGPU is missing or fails | Compiles in CI; loads on the iPhone |

**Gate G1.** On the reference phones, WebGPU build, high quality off, flying
the full Sierra and Coast exports along the route at the speed of the JS
fly-camera baseline from WP 0.8:

- frame time no worse than the JS game in the fly camera on the same phone
  (the Rust side draws stand-in materials for unported kinds, so this is a
  floor on its cost, not the final figure: the budgets are checked again at
  the exits of M3 and M7);
- no tab kill, no strobing or corrupt frames, in ten minutes and ten scene
  reloads;
- the wasm under 16 MB after gzip (10 MB when G1 was passed; D675);
- time from navigation to the client's first rendered frame within 5 s of
  the JS game's time to its menu;
- no frame over 50 ms after warm-up;
- on desktop, the gates of 2.3 and 2.4. On the phones the owner looks at the
  same material scenes for anything visibly wrong.

Scene download time and memory are not compared with the JS here: loading a
multi-hundred-megabyte export is not how the finished game will get its
world.

| Outcome | Next |
|---|---|
| Pass | Continue. WebGPU is the web target. Keep the WebGL2 build compiling until cutover. |
| Safari WebGPU fails, WebGL2 build passes | Ship both builds; the page picks. Raise the Safari bugs upstream. Shaders stay within what both accept. Tell the owner: this weakens "WebGPU required" for iPhones. |
| Bevy fails on both | Stop and decide with the owner: fallback renderer on wgpu using the same scene data (SPEC 2), with an estimate. |

**Owner:** run the build on the phones and report; approve the outcome.

---

## M3. World generation and Level 1

**Goal:** Sierra to the City generated by Rust, matching the JS export.
Size XL (about 11,000 lines).

| WP | Ports | Gate |
|---|---|---|
| 3.1 | `three_geom`: the three.js generators and merge (SPEC 5.2) | Each generator against a three.js dump |
| 3.2 | `mp_canvas`: the Canvas 2D subset and text (SPEC 5.3); the font gallery | The shared textures in `world/textures.js` within threshold |
| 3.3 | Builders: `valley/Builder`, `beach/ColorBuilder`, `city/geom`, `Road.js` extrude; `WorldBuild`, `Animator`, night parameters; the job list | Unit tests on builders |
| 3.4 | `Terrain.js`, `TerrainMesh.js`, the colouriser | L2 heights; L3 terrain digest |
| 3.5 | `Road.js`, `Sky.js` (parameters and keys), `Sea.js`, `World.js` | L3 road digest; L4 road and sky stations |
| 3.6 | `Mountain.js` and its material kinds | L3, L4 for zone 0 |
| 3.7 | `Valley.js`, `valley/*` | L3, L4 for zone 1 |
| 3.8 | `City.js`, `city/*` (freeway, textures, geometry) | L3, L4 for zone 2 |
| 3.9 | Remaining material kinds for Level 1, animators (waterfall, flag, windpumps, traffic streams, aircraft lights); `runout` and the opposite carriageway computed by the ported scenery | L4 all Sierra stations; the world data equals the WP 0.4 dump; budgets of SPEC 6.6 on the phones; L5 |

**Order:** 3.1, 3.2 and 3.3 first and together. Then 3.4, then 3.5. Then
3.6, 3.7 and 3.8 together, one agent each. 3.9 closes.

**Owner:** pick the fonts from the gallery; review the Sierra report.

**Exit:** the fly camera runs the whole of Level 1 in the Rust client;
digests match; every station is within threshold or signed off.

---

## M4. A playable race

**Goal:** Level 1 can be raced from countdown to results with a keyboard, on
desktop web and native.

| WP | Ports | Gate |
|---|---|---|
| 4.1 | `CarModel.js`: thirteen kinds, detail levels, far model, light setters. Size M | L3 digests per kind; dimensions equal `mp_sim::dims`; L4 against `tools/car-test.html` views |
| 4.2 | Session and tick loop (loopback), interpolation, `Vehicle.sync` | Motion is smooth at 60 and 120 Hz; no drift from the simulation |
| 4.3 | `CameraRig.js`, the intro camera | L4 stations in chase, far and bumper views |
| 4.4 | `Effects.js`: smoke, sparks, skids, flames, headlight pools; the headlight spot | L4 staged effect scenes |
| 4.5 | Keyboard input, the input layer and ramp | L1 (`input.test.js`) |
| 4.6 | Minimal flow: start a race, countdown, finish, a plain results list; traffic and rivals drawn with far-model switching | `traffic-lod` e2e test |

**Owner:** drive it. This is the first feel check: does it handle like the
JS game? Report anything that feels different; the traces say whether it is.

**Exit:** a full race on Level 1, start to finish, in the browser and
native.

---

## M5. Audio

**Goal:** the game sounds the same. Size L (about 3,500 lines). Can run any
time after M2.

| WP | Ports | Gate |
|---|---|---|
| 5.1 | The facade and the null, web and native backends | Backend conformance tests; strict validation mode |
| 5.2 | `samples.js`, noise beds, impulse responses, engine cycles | L2 against the JS arrays |
| 5.3 | Buses, `Gate`, volumes, pause and unlock rules, the iPhone audio session | L1 (`audio-session`); `audio` e2e tests |
| 5.4 | Engine for the five cars, turbo, electric, damage | L4 offline band comparison; call log matches JS |
| 5.5 | Environment voices and one-shots | Same |
| 5.6 | `Music.js`, `tracks.js`: sequencer, instruments, seven songs | L1 (`music.test.js`); L4 offline comparison per song |
| 5.7 | Radio: lines, clips, the radio bus, the burble; sirens and pursuit calls | L1 (`radio`, `pursuit-audio`) |
| 5.8 | The Music player screen | `music-player` e2e tests, adapted |

**Owner:** listen on the iPhone with the Silent switch on, and on desktop.

---

## M6. Menus, HUD and every input

**Goal:** the complete front end. Size M to L (about 3,000 lines of JS and
440 of CSS).

| WP | Ports | Gate |
|---|---|---|
| 6.1 | The widget module, fonts, layout breakpoints, the store | Unit tests on the store with the JS keys |
| 6.2 | Screens: loading, menu, pause, results, controller setup | `menu`, `race-flow`, `race-button` e2e |
| 6.3 | HUD: all elements, the dials and minimap, speed lines, toasts, banners | L1 (`hud.test.js`); L4 HUD screenshots |
| 6.4 | Gamepad: bindings, remapping, menu navigation, rumble | L1 (`gamepad.test.js`); `gamepad` e2e |
| 6.5 | Touch controls: stick, slider, buttons, auto gas | L1 (`touch.test.js`); `touch-controls`, `analog-controls` e2e |
| 6.6 | Tilt, fullscreen and landscape lock through the gesture bridge; visibility pause | L1 (`tilt.test.js`); `tilt` e2e |
| 6.7 | The full test bridge (`window.__mp`, begun in WP 2.5) and the harness `target` option | The e2e suites run against the Rust build |
| 6.8 | Native: window state, F11, `--query`, `--smoke-test` | Smoke test in CI |
| 6.9 | The level viewer (SPEC 8.6): free fly, orbit, overview and ride cameras; the panel; keyboard, mouse, pad and touch; links that carry the pose. Rust only (D679) | `viewer` e2e; the owner reviews every level with it on desktop and phone |

**Order:** 6.1 and 6.7 first. Then 6.2 to 6.6 in parallel.

**Owner:** review the menu and HUD beside the JS ones early in this
milestone, since this is where an in-engine UI may fall short of the CSS.
Play Level 1 on the phone with the stick and with tilt.

**Exit:** the e2e suites are green against the Rust build, desktop and phone
emulation.

---

## M7. The other levels

**Goal:** all six levels. Each is independent: one agent per row.

| WP | Level | Ports | Size |
|---|---|---|---|
| 7.1 | Coast Highway | `Coast.js`, `coast/kit`, `Beach.js`, `beach/*`, `Harbor.js`, `harbor/*` | L |
| 7.2 | Downtown Streets | `Streets.js`, `streets/*` | L |
| 7.3 | Desert Run | `Desert.js`, `desert/*` | L |
| 7.4 | Seaside Raceway | `Raceway.js`, `raceway/*`; the photo drape and loose-ground mask in the terrain kind; start lights; lap HUD | M |
| 7.5 | Night City Cruise | The loop variants in `City.js` and `city/freeway.js`; chunk cut-off; cruise scoring HUD | S |

Each row's gate: L3 digests, L4 stations, the `levels` e2e test (and
`circuit` for 7.4), the budgets in SPEC 6.6, then L5.

**Owner:** review each level's report; drive each on the phone.

---

## M8. Hot Pursuit on screen

**Goal:** the pursuit mode complete. The rules are already in the
simulation (WP 1.6).

| WP | Ports | Gate |
|---|---|---|
| 8.1 | Police models and liveries, light bars, glow billboards, the shared light, the flash option | L4 staged scenes |
| 8.2 | Roadblocks, sawhorses, spike strips; damage smoke, rim sparks | L4 staged scenes |
| 8.3 | Pursuit HUD (stars, bust and evade bar, hold card, damage, radio line), results tiles, the mode switch | `pursuit` e2e |
| 8.4 | Pursuit audio hook-up: sirens, mood, radio, stingers | `pursuit` e2e (radio prefetch and fallback) |

---

## M9. Cutover

**Goal:** the Rust build replaces the JS game.

| WP | Work | Gate |
|---|---|---|
| 9.1 | Tuning on the phones against the budgets; wasm size work | SPEC 6.6 met on all reference devices |
| 9.2 | Full parity review: every level's report, every e2e suite, the deviations list | L5 |
| 9.3 | Native app: `play.sh` runs the Rust binary; desktop entry; Electron removed | Smoke test |
| 9.4 | `tools/og-image` against the Rust build; README rewritten | The preview picture regenerates |
| 9.5 | Swap: the Pages workflow (already on GitHub Actions, D1101) publishes Rust at the site root and JS under `/legacy/`; JS source moves to `legacy/`; records carry over | Best times from the JS game show in the Rust menu |

**Owner:** final sign-off; a week of play before the swap.

**Exit:** tag `v1.0`. From here, new features are Rust only.

---

## M10. Multiplayer: race together

**Goal:** two to eight people on a LAN, each in a browser tab, racing the
existing levels (SPEC 9).

| WP | Work | Gate |
|---|---|---|
| 10.1 | Design note: rubber-banding with several humans, grid order, AI fill, drop-outs, results (draft: `MULTIPLAYER.md`) | Owner approves |
| 10.2 | Protocol, the transport trait, loopback and the lossy test transport | Unit tests |
| 10.3 | Session: lobby, authoritative stepping, input relay, snapshots | Eight headless clients for an hour with latency and jitter: no desync that a snapshot does not repair |
| 10.4 | Client: prediction, rollback, clock sync, correction smoothing | Two cars side by side at 50 ms simulated latency: contact looks the same on both |
| 10.5 | `mp-host`: static files, wss, TLS options, the port rule; `serve.sh` switches to it | `proj doctor` clean; phones join over https |
| 10.6 | Multiplayer simulation rules from 10.1; names and colours | Tests |
| 10.7 | Lobby and results UI; join by address or QR code | Multi-tab e2e: four tabs complete a race |

**Owner:** a race with real people, over the tailnet's https front first;
decide how guests outside the tailnet join (SPEC 9.5).

---

## M11. Multiplayer: tab hosting and the rest

- WebRTC transport through `matchbox_socket`; a signalling service; the
  native host as a peer; a tab as the authority.
- Players load the page from GitHub Pages and need no certificate.
- Hot Pursuit and the cruise loop with several players (needs its own
  design: who the police chase, shared or separate heat, cruise scoring).

---

## M12 and beyond: expansion tracks

Each needs its own design document before work starts. They are listed to
show what the architecture is keeping room for. **The order after M11 now lives in
`docs/vision/ROADMAP.md`** (milestones M12 onward), which supersedes this
table; the vision behind it is `docs/vision/WORLD.md`.

| Track | What | Builds on |
|---|---|---|
| a. Renderer upgrades | Cascaded shadows, real local lights, better anti-aliasing: what WebGPU allows once exact parity is no longer the goal | M9 |
| b. Free-roam driving | The arcade handling off the road corridor: ground from a height field, collision with static geometry | SPEC 4.5 seams |
| c. RL environment | `mp_sim::Env`, batched stepping, `mp_py`; first task: train rivals on the existing tracks | M1 only |
| d. Open world | Chunked world generation and streaming of scene data; a road network instead of one route | b |
| e. Realistic physics | Sim handling (per-wheel tyre model) and soft-body tyres as further vehicle models: `docs/vehicle-dynamics/SPEC.md` | b |
| f. Authored assets | glTF models, image textures and audio files beside generated content | M9 |
| g. New modes | Weapons and pickups, kart-style items, playing as the police | M10 |

---

## What can go wrong, and when we find out

| Question | Answered at |
|---|---|
| Does Bevy through WebGPU run this on an iPhone? | G1 (M2), with real levels |
| Can Rust reproduce the JS simulation bit for bit? | WP 1.3, on the physics alone, before the rest of M1 |
| Can the scenery be matched without endless tuning? | M3: Level 1 is the largest and proves the method |
| Does it feel the same to drive? | M4 |
| Is the in-engine UI good enough? | Early M6 |
| Does rollback netcode hold up with eight cars? | WP 10.3 and 10.4 |
