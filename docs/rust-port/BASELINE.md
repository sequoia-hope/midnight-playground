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
