# Midnight Playground (formerly Midnight Racer): working rules for agents

The game is **Midnight Playground**, in Rust; its crates are `mp_*`
(DECISIONS D1100). The repository is `sequoia-hope/midnight-playground` and
Pages serves the Rust game at https://sequoia-hope.github.io/midnight-playground/
(the cutover, D1112). The JS game keeps the name Midnight Racer and is the
legacy version, under `legacy/` on Pages. The local checkout, the `proj`
name and the tailnet path are `midnight-playground` too (renamed 2026-10-10).

Two games live in this repo:

- **The JS game** (repo root: `index.html`, `src/`, `vendor/`, `test/`). It is
  the legacy version (Pages serves it under `legacy/`), and it is **frozen** except for bug
  fixes the owner reports, and parity hooks during roadmap M0. After the
  `js-reference` tag, hooks are closed too.
- **The Rust port** (`Cargo.toml`, `crates/`, `xtask/`, `tools/parity/`,
  `parity/`). The plan is `docs/rust-port/SPEC.md` (what and how it is
  judged) and `docs/rust-port/ROADMAP.md` (order, work packages, gates). Read
  both before starting a work package, plus the inventory section for the JS
  you are porting.

## Principles (SPEC 1.1)

**Since D1111 (2026-10-07) parity with the JS game is no longer the goal.**
The owner judges the Rust game on its own; principles 1 and 2 below now
apply only as history and as regression checks that still pass. Principles
3 to 6 stand: multiplayer relies on determinism.

1. **The JS game is the oracle.** Parity is measured by tests, numeric
   traces, geometry digests and side-by-side pictures, not by "looks right".
2. **Port, don't improve.** Same structure, names, constants, order of
   operations and order of random draws as the JS. Keep the comments that
   explain why. A behaviour change is a deviation: record it in
   `docs/rust-port/DEVIATIONS.md` with the reason.
3. **The simulation is a library**: no engine, renderer, clock or global
   state; it steps a plain `SimState` by one fixed tick of 1/120 s.
4. **World generation is a library**: level in, engine-neutral scene data out.
5. **One implementation for every platform**; platform differences sit
   behind small traits.
6. **Deterministic by construction**: fixed tick, seeded streams in the
   state, and every inexact math function through `pk_math`'s kernel (the
   kit's, `../game-engine/crates/pk_math`, D1231). Never
   call `f64::sin` and friends, `powi`, or `mul_add` in the simulation or
   world generation (`check-deps` rejects them).

A choice the spec does not cover: add it to `docs/rust-port/DECISIONS.md`
and carry on with the option that best preserves parity.

## Commands

| What | Command |
|---|---|
| Build and test the workspace | `cargo test --workspace` |
| Lints, as CI runs them | `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings` |
| Crate dependency rules (SPEC 3.2, `deps.toml`) | `cargo xtask check-deps` |
| Web build into `dist/next/` | `cargo xtask web` (add `--release` for the optimised build) |
| Wasm size report | `cargo xtask size` (after a release web build) |
| Frame times of the Rust client, desktop, phone or throttled (BASELINE; the JS game's: `perf-baseline.mjs`) | `node tools/parity/rust-perf.mjs --level sierra --race [--backend webgl2] [--phone] [--throttle 4]` (on a laptop over ssh: `--headed --angle gl --size WxH --dpr N`, see BASELINE "The laptop itself") |
| CPU profile by Bevy system (needs the names-kept build: `cargo xtask prof`) | `node tools/parity/rust-prof.mjs --level sierra --backend webgl2 --throttle 4` (`--from <.cpuprofile> --callers <regex>` to dig) |
| WebGL2 calls a frame, and the GPU's time by pass and shader program (the JS game's figures are in BASELINE) | `node tools/parity/gl-calls.mjs` (`--by-program`, `--angle gl`, `--headed`, `--sub-frag 're=>to'` for what-if shader edits, `PK_DUMPIO=1 INTEL_DEBUG=perf` for Mesa's compile notes) |
| Two builds of the Rust client compared pixel for pixel (WebGL2, four levels; `dist/prof/` is the usual before) | `node tools/parity/build-diff.mjs --a prof --b next` |
| The Rust page's console when a build does not come up | `node tools/parity/web-console.mjs --backend webgl2` |
| A headless race (results, `--trace`, `--state-at`, `bench`) | `cargo run --release -p mp_sim --bin mp-sim -- race --level sierra` |
| Native client | `cargo run -p mp_game` |
| The signalling relay as a Cloudflare Worker, under the relay's own tests (`npm install` in `tools/signal-worker` first; deploy steps in its README) | `MP_SIGNAL_WORKER=1 cargo test -p mp_host --test signal --test rtc` |
| The open world's plan, measured (`resolve` rewrites what the planner draws; `corridor <id>` writes a route's road for a level, `docs/vision/peninsula.md`) | `cargo run --release -p mp_atlas -- report` |
| Rebuild the atlas from real data (python3 with numpy, Pillow, shapely, pyarrow) | `python3 tools/atlas/build.py` |
| The ground and land cover under the open world's corridors (stages 2 and 4a of `docs/vision/peninsula.md`) | `python3 tools/atlas/ground.py coast-south hwy-92 skyline-south hwy-84 --margin 1200 --step 16 --name peninsula` |
| Drive the web client round the loop with the keyboard | `node tools/parity/drive-branch.mjs --level peninsula [--phone] [--secs 900]` |
| Drive the web client along the open world's roads with the keyboard, watching for a hang (console, ticks, frame time) | `node tools/parity/drive-branch.mjs [--phone] [--query "touch=1&assist=2&physics=player"]` |
| The police radio's script (every clip, for `tools/radio-voice/render.py`) | `cargo run -p mp_audio --example radio_lines` |
| JS unit tests | `npm run test:unit` (and `npm run test:unit:kernel`, with the parity kernel) |
| Rebuild and check the parity kernel | `cargo xtask kernel` (CI: `cargo xtask kernel --check`) |
| JS browser tests | `npm run test:e2e` (headless Chrome on the GPU, ~3 min) |
| The same browser suites against the Rust build | `cargo xtask web --release && npm run test:e2e:rust` (one Chrome at a time; prints the suite table) |

The build tooling (`cargo xtask web`, `size`, `prof`, `check-deps`,
`imgdiff`) is Playground Kit's `pk_xtask` (D1233): `xtask/src/main.rs` is
its `WebBuild` config plus the parity commands. The headless-Chrome tools
(`rust-perf.mjs`, `rust-prof.mjs`, `gl-calls.mjs`, `build-diff.mjs`,
`web-console.mjs`, `e2e/harness.mjs`, `lib/server.mjs`) are thin wrappers
with this game's defaults over `../game-engine/js/*.mjs`, which read the
status global named by `--global` (`__mp` here). `crates/mp_game/web/
index.html` is generated from the kit's loader page by
`tools/parity/gen-index.py`: edit the branding and hook blocks there.

Toolchain: stable Rust with the `wasm32-unknown-unknown` target
(`rust-toolchain.toml`), `wasm-bindgen-cli` at exactly the version of the
`wasm-bindgen` crate in `Cargo.lock`, and `wasm-opt` from binaryen at
exactly the version `xtask/src/main.rs` pins (116, as CI installs it);
`cargo xtask web` checks both.

## Crate rules (SPEC 3.2)

```
pk_math ← mp_track ← mp_levels ← mp_sim ← mp_net ← mp_host   (pk_math is the kit's kernel, noise and vectors, D1231)
pk_math ← mp_vdyn ← mp_sim   (vehicle dynamics: the sim car's body, tyres, drivetrain)
mp_scene ← mp_worldgen   (also pk_canvas and pk_math from the kit, mp_track, mp_levels; the fonts are mp_worldgen::fonts, D1232)
mp_exhaust ← mp_audio   (mp_exhaust has no dependencies; it is also its own wasm)
mp_atlas   (real geography and the open world's plan; no dependencies yet)
mp_builder uses mp_worldgen (the open world's section builder; its own wasm in the web's worker)
mp_game uses mp_sim, mp_net, mp_scene, mp_worldgen, mp_audio
```

Only `mp_game` depends on Bevy. The simulation crates (`mp_track`,
`mp_levels`, `mp_vdyn`, `mp_sim`, and `pk_math` in the kit) have `#![forbid(unsafe_code)]` and no
clock, threads, `rand`, or hash maps. `deps.toml` at the repo root holds
the rule for every crate; a new crate needs one there.

## JS semantics to keep (SPEC 4.2)

`Math.sign(-0)` is -0; `Math.round` is floor(x + 0.5) with its edge cases;
`Math.max`/`min` propagate NaN and order the zeros; `x || d` treats 0 as
missing; `Array.prototype.sort` is stable (`sort_by`, never
`sort_unstable_by`); round to `f32` wherever the JS stores into a
`Float32Array`; iterate in JS insertion order, never a sorted or hashed map.
When a trace diverges, it is a port bug until shown otherwise: find the first
differing field at the first differing tick.

## Parity data

`parity/README.md` explains the layout (`parity/golden/` in git,
`parity/cache/<js-tree-key>/` regenerated) and the hooks in `src/parity/`
that the JS game gains for its reference runs (`?kernel=1`, `?fixeddt=1`,
`?quant=1`, `?seed=N`, or `?parity=1` for all). Every capture is taken with
the kernel on.

## Viewing output

Only through the registered server: `proj up midnight-playground` (or
`./serve.sh`). It serves the repo root laid out as Pages lays out the site
(D1197): the Rust build (`dist/next/`, once built) over the root, so `/` is
the Rust game and `/dist/next/` still works; the JS game under `/legacy/`;
the tools and parity reports where they are (`/parity/report/`). Phones and
other machines use the tailnet https front (the registry's `web_url` for
this project), which is a secure context, so WebGPU and tilt work there.

**Never start another server, and never write a port number** into a script,
config, default or doc. A program that needs a port takes `--port`, then
`$PORT`, then `proj port`, then fails loudly. Every URL the client uses is
relative, because the game is always served under a sub-path.

## Working on a package (ROADMAP "Working rules")

1. Read the JS files named in the package in full, and the inventory section.
2. Write the parity test first where a golden exists; port until it passes.
3. Check your own work: run the tests, render headless, look at the result,
   compare with the reference.
4. Commit and push straight to `main` (no branches): imperative subject, a
   prose body. Build output (`dist/`, `target/`, `parity/report/`) stays out
   of git. `.github/workflows/pages.yml` builds and publishes on every push
   to `main` (D1112): the Rust build at the site root, the JS game's tracked
   files under `legacy/`, and `dist/next/` forwarding to the root. The web
   build is self-contained (`cargo xtask web` copies the fonts, Seaside's
   survey and photo and the radio clips into it), so it works at
   `dist/next/` locally and at the root on Pages: never reach above it with
   `../../` from the Rust page.

**Parallel work.** Packages run in parallel only with disjoint files. A
shared module has one owner; everyone else may add exports to it but not
change what exists.
