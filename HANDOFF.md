# Handoff: before the move into the monorepo (2026-10-18)

## Current state

- `main` is clean and pushed (`75679ca`, "Run the tests under nextest").
  CI and Pages are green on it. Pages serves the Rust game at
  https://sequoia-hope.github.io/midnight-playground/ with the JS game
  under `legacy/`.
- The agent worktrees and their `worktree-agent-*` branches were removed
  on 2026-10-18 (all merged). The two branches left, `hot-pursuit` and
  `origin/claude/optimistic-franklin-ijm0fb`, are merged into `main` too.
- The last package to land was the Playground Kit carve (D1230 to D1234):
  `pk_math`, `pk_canvas`, `pk_xtask`, `pk_shell`, the patched
  wgpu/wgpu-hal/bevy_ui_render and the headless-Chrome tools now live in
  the sibling repo `../game-engine` (`sequoia-hope/playground-kit`).

## In progress / waiting on the owner

- **Open world, stage 5 gate** (`docs/vision/peninsula.md`, D1219/D1220).
  The owner still has to try it: from Drive, find and play a race, a
  trial and the delivery. Known gap: Restart in the pause menu restarts
  the whole cruise, because the InputFrame flags byte is full.
- Smaller open-world gaps: 200 to 700 ms frame spikes on WebGL2 when a
  new section brings an unwarmed pipeline. Traffic, rivals and police run
  on road 0 only. The minimap doesn't show the branches. Hot Pursuit on
  the loop still needs radio clips for OVER THE HILL and SKYLINE.
- **One old stash** (`stash@{0}`, 2026-09-30) holds a JS `hud.css` tweak
  (`--steer-top` for the speedo). It predates the thumb-stick commit and
  is probably superseded. Drop it or apply it; it is not current work.

## Known issues

- `mp_host`'s http test is flaky when the machine is loaded during
  `cargo nextest run --workspace`. It passes when run alone.
- Playground Kit's `dca0b2d` (D9: "wgpu destroys dropped buffers and
  textures on WebGPU") was pushed on 2026-10-18, so CI's clone of the kit
  now matches local builds. The first CI run with it is the check.

## Concerns about the move

1. **The `../game-engine` sibling layout is load-bearing.** Several
   places reach it by relative path:
   - `Cargo.toml`: the `pk_*` path dependencies and the
     `[patch.crates-io]` wgpu/Bevy patches.
   - `tools/parity/*.mjs`, `e2e/harness.mjs` and `lib/server.mjs`: these
     import `../../../game-engine/js/*`.
   - `ci.yml` and `pages.yml`: these clone the kit to
     `$GITHUB_WORKSPACE/../game-engine`.

   Either keep the two repos side by side in the monorepo, or change all
   of these places in one commit. `tools/parity/gen-index.py:10` also
   hardcodes `/home/sequoia/Software/game-engine/...` as an absolute path.
2. **Cargo workspaces.** This repo is its own workspace, with its own
   `[patch.crates-io]` and `rust-toolchain.toml`. If the monorepo gets a
   root `Cargo.toml` workspace, this one has to be excluded from it or
   merged into it. Cargo honours patches only at the workspace root, so a
   nested workspace silently loses the wgpu/Bevy patches. A merged
   workspace also takes on the `deps.toml`/`check-deps` rules.
3. **GitHub Actions and Pages.** Actions only run workflows from the repo
   root's `.github/workflows/`. Pages is tied to the
   `sequoia-hope/midnight-playground` repo, and the game's URL, the
   relative-URL rule and the `legacy/` layout all assume that. If the
   monorepo becomes the published repo, the workflows have to move and
   the Pages URL changes. If it doesn't, decide which repo is the source
   of truth for pushes to `main`.
4. **Git worktrees break on a move**, because git registers them by
   absolute path. None are left now. If an agent creates one before the
   move, remove it first.
5. **Ignored local state that a fresh clone won't have.** `target/` is
   194 GB. A moved `target/` mostly rebuilds anyway, because of absolute
   paths. `parity/cache/` (5.4 GB) regenerates, but slowly.
   `tools/atlas/cache/` and `tools/seaside/cache/` are the downloaded
   OSM/lidar/NAIP inputs. Also `recordings/`, `dist/`, and the
   `node_modules/` at the root and in `tools/signal-worker/`. Move these
   with `mv` rather than relying on git.
6. **Port registry and serving.** `~/scripts/launcher.toml` registers
   `midnight-playground` with `dir = /home/sequoia/Software/midnight-playground`.
   It owns the web port and the `stream`/`signal` extras, and
   `serve.sh` resolves the port through `proj port` from the current
   directory. After the move:
   - Update the registry's `dir`.
   - `proj down`, then `proj up midnight-playground`.
   - Run `proj doctor`.

   The tailscale front at `/midnight-playground/` is re-pointed on each
   start. The dev server is running now (`tools/serve.py`); stop it
   before moving.
7. **Agent memory is keyed by path**:
   `~/.claude/projects/-home-sequoia-Software-midnight-playground/memory/`.
   Copy it to the directory for the new path, or later sessions lose the
   handoff notes on the open world, radio voice, profiling, the X1 laptop
   and Seaside data.
