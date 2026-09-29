# Midnight Racer

A Need-for-Speed-style street racer that runs in the browser, and also as a
desktop app. It's built with three.js and has no build step and no asset files:
the terrain, cars, textures, music and engine sound are all generated in code.

## Levels

**Level 1: Sierra to the City.** A 9.2 km sprint against five rivals, from
late-afternoon sun to midnight:

1. **Sierra Pass**: rocky canyon walls, three stacked hairpins, a summit
   lookout and a descent with views over the valley.
2. **Old Mill Valley**: a two-lane farm road at sunset. It passes old farmsteads,
   crosses a creek bridge and goes over crests that can send you airborne.
3. **Interstate 9**: the freeway through downtown Meridian at night, with a
   viaduct and a tunnel.

**Level 2: Coast Highway.** A 7.5 km sprint at dawn with the sea on your left
the whole way:

1. **Hollow Point**: a cliff road high above the ocean.
2. **Seabright**: a beach-town boulevard as the sun comes up.
3. **Port Meridian**: over the harbour bridge and into the container docks.

**Level 3: Downtown Streets.** A 5.2 km street race through Meridian at
midnight, against five rivals. The route follows the city grid, so every
corner is a right angle at a crossing. The race has closed the side streets
with barriers:

1. **Neon District**: shopping streets under neon signs and strings of
   lanterns, where an elevated train crosses overhead.
2. **Nob Hill**: steep streets of painted rowhouses. Every crossing is level,
   so a fast car takes off from each one on the way down.
3. **Financial District**: a sprint between the office towers to the finish.

**Level 4: Desert Run.** A 7.4 km sprint from golden hour to moonrise, against
five rivals:

1. **Red Rock Canyon**: a two-lane road between banded sandstone walls, through
   the narrows, under a natural arch and past an amphitheatre of hoodoos.
2. **Route 66**: the open highway across the basin at sunset. There's a ranch
   fence, telephone poles, Joshua trees and the neon Oasis motel, diner and gas
   station. Wash crossings throw a fast car into the air, and a freight train
   runs alongside for you to pass.
3. **Silver Lake**: flat out across a dry lake bed after dark, between cones,
   flags and flares, past a spectators' camp to a floodlit finish.

**Night City Cruise.** An endless 14 km freeway loop through the city at
midnight, with heavy traffic and no finish line. Speed scores points, and near
misses build a multiplier of up to ×10, which you lose if you crash. End the run
from the pause menu to see your score.

## Cars

| Car | Character |
| --- | --- |
| Vento GT | A balanced all-rounder with a flat-plane V8 |
| Brawler 69 | Big power and a loose tail, with a cross-plane V8 |
| Stiletto R | Grip and precision, with a V10 |
| Kestrel RS | A turbo four-wheel-drive rally hatch. It launches hard and holds a slide well, but its top speed is lower. You hear the turbo spool up and the blow-off valve when you lift |
| Ion Arc | Electric, with instant torque and the quickest launch. It has one gear and a lower top speed. Braking and lifting off charge the boost tank, and the rev counter becomes a power meter |

## Run it

Play it online at **https://sequoia-hope.github.io/midnight-racer/**. GitHub Pages
serves `main` as-is (there is no build step), so every push to `main` goes live
within a minute or two.

To run it locally:

```sh
proj up midnight-racer      # or ./serve.sh — both use the registered port
proj url midnight-racer     # prints the URL to open
```

## Controls

| Key | Action |
| --- | --- |
| W / ↑ | Throttle. Get on it in the last moment before GO for a perfect start |
| S / ↓ | Brake, then reverse |
| A D / ← → | Steer |
| Space | Handbrake. Drifting fills the nitro tank, and so do near misses and big air |
| Shift / N | Nitro |
| C | Change camera (chase, far, bumper) |
| B | Look back |
| R | Put the car back on the road |
| M | Mute or unmute music. The menu and the pause screen have separate music and SFX volume sliders |
| T | Next music track. There are seven; each level starts on its own, and the playlist moves on when a song ends. The pause screen has a Next track button, and the menu a Track picker |
| Esc / P | Pause |

A gamepad also works: RT and LT for throttle and brake, A for nitro, X or RB for the
handbrake, Y for the camera, and Start to pause.

### Phones and tablets

On a touch screen the game shows on-screen controls while you drive: steering
buttons bottom-left; brake, gas, handbrake (DRIFT) and nitro bottom-right; and
reset, camera and pause at the top left. Every finger is tracked, so you can
steer and hold the gas at the same time, and slide a thumb from gas to brake
without lifting it. Nitro holds the gas down for you.

Hold the phone sideways; portrait works but shows less of the road. Pressing
Race goes fullscreen and locks landscape where the browser allows it (turn this
off with the Fullscreen option). The menu also has **Auto gas**, which keeps
the throttle down unless you're braking. High quality is off by default on
touch screens. Switching away from the browser pauses the race. You can also
add the game to your home screen, where it opens fullscreen in landscape.

## Layout

```
src/
  main.js              renderer, bloom, menus, main loop, debug hooks
  levels/*.js          level data: route segments (or a closed loop), zones and
                       landforms, time of day, traffic rules, rivals
  track/Track.js       samples the route every metre; frames, racing line, AI speed
                       profile; loops wrap around
  world/Terrain.js     height field sculpted around the road; landforms: mountain,
                       valley, city, coast, beach, harbor, streets, canyon, desert,
                       playa
  world/Sea.js         open water for coastal levels
  world/TerrainMesh.js terrain tiles with LOD and stitched seams, triplanar rock
  world/Road.js        road surface, markings, guardrails, fences, barriers, viaduct
  world/Sky.js         sky dome, sun and moon, fog, time of day along the route
  world/Mountain.js    Level 1 pass: rocks, pines, start gantry, diner, signs, waterfall
  world/Valley.js      Level 1 farms, creek and bridge, orchards, poles
  world/City.js        city blocks and freeway furniture (Level 1 and the cruise loop)
  world/Coast.js       Level 2 cliffs: lighthouse, sea arch, cypress trees
  world/Beach.js       Level 2 beach town, pier, marina
  world/Harbor.js      Level 2 cable-stayed bridge and container port
  world/Streets.js     Level 3 street grid: kerbed blocks, shops and neon, rowhouses,
                       towers, signals, race barriers, elevated railway
  world/streets/       Level 3 textures: shop fronts, rowhouses, neon signs
  world/Desert.js      Level 4 canyon, arch, hoodoos, Route 66, railway, lake bed
  world/desert/        Level 4 parts: rocks, plants, fence, poles, train
  vehicles/            car models, player physics, rival AI, traffic, collisions
  game/                race flow, camera, HUD, effects, audio, input, touch controls
  game/audio/          the soundtrack: step sequencer and synth instruments, the songs,
                       and the drum kit and crash sounds pre-rendered at start-up
vendor/three/          three.js r180 (the build plus the few add-ons used)
tools/                 check-track.js, car-test.html, audio-test.html, serve.py
```

## Dev hooks (URL parameters)

- `?s=2350&h=8&back=16&lat=0&yaw=0&pitch=-0.1&t=0.5`: a fly-along debug camera
  at a point on the track, with an optional fixed time of day `t` (0 to 1).
- `?level=sierra|coast|streets|desert|cruise`: choose the level.
- `?autostart=sports|muscle|super|rally|electric`: skip the menu and start a race.
- `?autodrive=1`: a simple autopilot drives the player car (for testing).
- `?timescale=2`: run the simulation faster.
- `?stats=1`: frame time, draw calls and triangle count.
- `?touch=1` / `?touch=0`: force the on-screen touch controls on or off (they
  appear automatically on touch screens).

To check a route after editing its level file, run `node tools/check-track.js <level>`
(or `npm run check-track`, which checks Level 1). It
reports each zone's length and height, the tightest corner, the steepest
grade, and any place where two parts of the road overlap.

## Desktop app

The game can also run in its own window, with no browser and no web server.
It uses Electron, and the project folder is served through a private
`app://` scheme, so no port is involved.

```sh
./play.sh              # first run installs Electron via npm, then opens the game
npm run app            # the same, if dependencies are already installed
./play.sh --dev        # enables F12 devtools
./play.sh --url-query stats=1   # passes URL parameters to the game
```

**F11** toggles fullscreen and **Ctrl+Q** quits. The window's size, position
and fullscreen state are remembered between runs.

On Ubuntu, AppArmor blocks Chromium's sandbox unless Electron's
`chrome-sandbox` helper is owned by root and setuid. When it isn't, `play.sh`
detects this and falls back to `--no-sandbox`; the app only loads its own
local files. You can force either behaviour with `MR_SANDBOX=1` or
`MR_SANDBOX=0`.

To add Midnight Racer to your applications menu, run
`tools/install-desktop-entry.sh`. It writes
`~/.local/share/applications/midnight-racer.desktop`; delete that file to
remove it.

`npm run app:smoke -- --no-sandbox --shot out.png` is a quick check of the
desktop build. It opens the game and waits for it to report ready, then
prints any console errors, saves a screenshot, and exits non-zero on
failure.
