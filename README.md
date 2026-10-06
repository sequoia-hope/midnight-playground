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

**Level 5: Seaside Raceway.** Three laps of a 3.6 km circuit in the
late-afternoon sun, against five rivals, with no traffic. It's a copy of
Laguna Seca, the circuit in the hills above Monterey Bay, built from survey
data (see [How Seaside Raceway was built](#how-seaside-raceway-was-built)):

1. **The Hairpin**: from the line down the pit straight, over the blind crest
   of Turn 1 and down into the hairpin.
2. **The Climb**: through Three, Four, Five and Six, then up the long straight
   to the top of the hill.
3. **The Corkscrew**: a blind left-right over the top that drops 18 m in
   150 m, down through the long left and Ten to the last hairpin and the line.

The HUD shows the lap, this lap's time and your best, and the results list
every lap. The menu card keeps your lap record. Off the tarmac, the run-off
is the real one: the paved run-off drives like the road, and the dirt and
grass slow you down. Tyre walls and concrete walls with catch fencing stand
where the real barriers are.

**Night City Cruise.** An endless 14 km freeway loop through the city at
midnight, with heavy traffic and no finish line. Speed scores points, and near
misses build a multiplier of up to ×10, which you lose if you crash. End the run
from the pause menu to see your score.

## Hot Pursuit

Every sprint level can also be raced as **Hot Pursuit**: pick it with the
Race / Hot Pursuit switch on the level card (the choice is remembered per
level). It's the same race against five rivals, with the police on the road.

- Patrol cars wait on the shoulder and give chase when a racer blasts past.
  More join from behind, and on two-way roads some come the other way and
  U-turn as you pass. They chase whoever is nearest, but mostly you, so
  rivals get pulled over too.
- **Heat** (the stars at the top) rises the longer the chase goes on, and
  when you hit or take out police cars. Each zone caps it, so the first
  zone stays calm. Higher heat brings more and faster units, and new
  tactics: PIT manoeuvres (heat 2), rolling blocks and roadblocks (3),
  boxing you in and spike strips (4), and heavy roadblocks whose gap is
  closed by barriers you have to smash through (5).
- Break line of sight (round corners, in a tunnel, behind the canyon walls)
  and the **evade** meter fills. When it's full you've escaped, and the
  police go back to patrol until they spot you again. Out on open ground,
  on Route 66 and the dry lake, they can always see you.
- **Busted**: if you crawl along with a police car right beside you, the
  bust meter fills. When it's full you're held on the spot for a penalty
  (5.5 to 11.5 s, longer at higher heat). The race clock keeps running and
  the rivals drive on. Then you're put back on the road ahead of the
  police, with a few seconds' grace. Reset (R) is locked while the bust
  meter is filling.
- **Damage**: your car takes damage from crashes into cars and walls
  (police rams wear it down more slowly than a crash into traffic). Past
  half damage it smokes and loses power. At 100% it's **wrecked**: an 8 s
  penalty hold, then back on the road, repaired.
- Spike strips shred your tyres for 10 s: less grip, a lower top speed and
  sparks off the rims. Ramming a police car hard enough takes it out
  (a **takedown**).
- The results screen adds your busts, wrecks, takedowns, penalty time and
  top heat. Hot Pursuit winning times are kept separately from normal ones.
- Dispatch talks you through the chase over the police radio (the words
  show at the bottom of the screen too): the pursuit starting and which way
  you're heading, units calling in and going down, roadblocks and spikes,
  the heat rising, busts.

The police lights flash red and blue. If strobing lights bother you, turn
off **Police lights flash** in the menu: the lights then glow steadily and
don't light up the scene.

### The radio voice

The dispatcher was made with [Qwen3-TTS](https://github.com/QwenLM/Qwen3-TTS):
its VoiceDesign model made a voice from a written description, and its Base
model cloned that voice to record every line, so all 200-odd clips are one
speaker. They're small MP3s in `audio/radio/` (16 kHz mono; the game's radio
bus keeps only 300 to 3000 Hz anyway), fetched when a pursuit race starts
and played through the radio bus between squelch clicks. A line with no
recording falls back to a synthesised burble.

- The lines are in `src/game/audio/radioLines.js`. Each clip is named after
  its words, so after changing a line only the new words need recording.
- `tools/radio-voice.html` (on the dev server) plays the candidate voices
  and every line, as recorded or through the in-game radio.
- `tools/radio-voice/voices.json` has the voice descriptions and says which
  one the game uses. `python tools/radio-voice/design.py [name…]` designs
  voices (`voices/<name>.flac`, the reference clip). `python
  tools/radio-voice/render.py` records whatever is missing and rewrites
  `audio/radio/index.json`: `--only <words>` re-records matching clips,
  `--seed N` gives different takes, and switching voices re-records
  everything. Both need a Python with the `qwen-tts` package, a CUDA GPU
  with about 5 GB free, node and ffmpeg.

## How Seaside Raceway was built

Everything about the circuit's shape comes from free survey data, fetched and
boiled down by `python3 tools/seaside/build.py` (numpy and Pillow; add
`--refresh` to download again). It writes `src/levels/seaside/circuit.js`,
`ground.js` and `photo.jpg`, about 1.5 MB between them, which the game only
loads when you pick the level.

- **The racing line**: the circuit's route relation in
  [OpenStreetMap](https://www.openstreetmap.org/relation/21195763) (© OpenStreetMap
  contributors, ODbL). OpenStreetMap sits about 1.2 m west of the survey
  (the aerial photo and the lidar agree to within 30 cm), so everything
  taken from it is moved by the shift that centres the lap on the photo's
  tarmac. Then the line itself is nudged, by up to 1.5 m, onto the middle
  of the tarmac between the edges the photo shows. It comes out 3,595 m
  round against the official 3,602 m.
- **The tarmac's width**: measured off the photo at every metre, between
  the white edge lines or wherever the asphalt meets dirt or kerb paint.
  It runs from 10.5 m between Five and Six to 15 m down the pit
  straight.
- **The road surface, its camber and the ground round it**: the USGS 3DEP
  1 m bare-earth lidar DEM (survey `CA_AZ_FEMA_R9_Lidar_2017_D18`, flown
  2018–19, public domain). The road's height is the lidar across the
  tarmac at every metre, so the crest at Turn 1, the climb and the
  Corkscrew's 18 m drop are the real ones. The lap spans 54.9 m of height,
  and the official figure is 180 ft. The camber is the lidar's cross slope. The run-off grade on each side
  comes from the lidar too, and so do the hills out to 3 km, at 4 m near the
  track and 16 m beyond.
- **Where the barriers are**: OpenStreetMap's walls. At every metre the
  script casts out to the nearest wall on each side, so the run-off is as
  wide as the real one. Where there's no wall, a tyre wall closes it off.
- **Buildings, grandstands, bridges, the pit lane and the infield lake**:
  OpenStreetMap too.
- **The ground and the oaks**: USGS NAIP aerial photos (public domain).
  Within 160 m of the lap the 60 cm photo itself is draped over the ground
  (graded warmer and richer for the game's sun), so the paved run-off, the
  dirt, the green strips past the kerbs, the paddock and the paths are
  where they really are. Further out its colour paints the terrain, and
  the dark crowns in it place the coast live oaks.
- **Paved or loose run-off**: the photo again. Grey (asphalt, concrete) and
  painted ground is paved, and the car drives on it as on the road; tan
  and green (dirt, gravel, grass) is loose and slows it. About half the
  ground within 50 m of the lap is paved, as it is at the real circuit.

It's a game track, not a laser scan. The lidar is bare earth with 1 m
cells, so there are no kerb heights, bumps or paint in it, and the kerbs go
where each corner's shape says they should.

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
within a minute or two. Links to it unfurl with a picture in Slack, iMessage,
Discord and the like (the `og:` tags in `index.html`).

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

### Gamepads

A controller works for the whole game, menus included. Out of the box it uses
the standard layout:

| Button | Action |
| --- | --- |
| Left stick | Steer (analogue, with a dead zone) |
| RT / LT | Throttle / brake, both analogue |
| A | Nitro |
| X or RB | Handbrake |
| B | Look back |
| Y | Change camera |
| Back | Put the car back on the road |
| Start | Pause |

**Menus.** Push the D-pad or the left stick and a highlight appears. Move it
to any button, car, level or option, and press A to choose it. On a slider or
a drop-down, A starts adjusting it, ◂ ▸ change it, and A or B finishes. B goes
back: from the pause screen to the race, from the results to the menu.
Start races from the menu and the results, and pauses and resumes a race.
Touching the mouse or the screen hides the highlight. A button still held as
a screen changes doesn't count: holding A on **Race** won't fire the nitro
when the race starts.

**Remapping.** While a controller is connected, the menu shows
**Controller setup** and the pause screen shows **Controller**. Both open a
screen listing every action and what it's bound to. A row lights up while
you hold its button, so you can check the layout. Choose a row, then press
the button or move the stick you want for it (Esc cancels, or wait 8
seconds). Any button, D-pad direction, stick direction or trigger can be
used, and digital steering (the D-pad, say) ramps in like the keys. Each
button does one thing, so giving a button to one action takes it off any
other. **Defaults** goes back to the standard layout. The map is saved per
controller, so two kinds of pad each keep their own. A controller the
browser doesn't recognise as standard shows numbered buttons and axes;
remap it once and it works like any other. The menus always use the
standard D-pad, stick, A and B, so a bad map can be fixed from the pad.

**Rumble.** The pad jolts on crashes, landings and nitro, gives a small
kick on each gear change and the countdown, buzzes over gravel, along a
wall and on spiked tyres, and shakes for takedowns and busts in Hot Pursuit.
The **Rumble** switch on the Controller screen turns it off. Rumble needs a
browser that can drive the pad's motors (Chrome, Edge and the desktop app
can); elsewhere it does nothing.

### Phones and tablets

On a touch screen the game shows on-screen controls while you drive:
steering on the left, the pedals bottom-right, and reset, camera and pause
at the top left. Every finger is tracked, so you can steer and work the
pedals at the same time.

Steering is analogue by default: a thumb stick. Put your left thumb down
anywhere on the left of the screen and that's the centre; slide it left or
right to steer, further for more lock. Slide past full lock and the centre
comes with you, so sliding back steers the other way straight away. The
stick (like tilt and a gamepad's stick) asks for a share of what the tyres
can do at your speed rather than a wheel angle: full travel is always a bit
more turn than they hold, so at 200 km/h the whole stick still steers, not
just its first few millimetres.

The pedals are one vertical slider for your right thumb. From the bottom:
**BRAKE** (full at the very bottom, lighter going up), a gap to coast in,
**GAS** (light just above the gap, flat out from the dashed line) and
**N₂O** at the top, which also holds the gas flat. A knob follows your thumb
and the slider fills from the gap up (gas) or down (brake) to show how hard
you're pressing. **DRIFT** is the strip beside it: slide your thumb right
onto it for the handbrake while its height keeps setting the pedals, so you
can drift on the gas with one thumb. A thumb that starts on the slider keeps
working it until you lift it, even if it wanders off the side.

The menu's **Steering** option switches to ◂ ▸ buttons (tap for a little
lock, hold for more) or to tilt (below), and **Pedals** switches to separate
GAS, BRAKE, DRIFT and N₂O buttons, where you can slide a thumb from gas to
brake without lifting it.

Hold the phone sideways; portrait works but shows less of the road. Pressing
Race goes fullscreen and locks landscape where the browser allows it (turn this
off with the Fullscreen option). The menu also has **Auto gas**, which keeps
the throttle down while your thumb is off the pedal slider (or, with pedal
buttons, unless you're braking). High quality is off by default on
touch screens. Switching away from the browser pauses the race. You can also
add the game to your home screen, where it opens fullscreen in landscape.
On an iPhone the game is heard even with Silent mode on, like a music app,
and anything else playing (a podcast, Spotify) pauses while it plays.

**Tilt** steering steers by turning the phone like a steering wheel. Level
is straight ahead, so there's nothing to calibrate, and the Tilt slider sets
how far you turn for full lock. While tilt is steering, a wheel on the left
shows the lock. Phones only send the motion sensor to https pages, so tilt
works on the GitHub Pages build but not on a plain `http://` address from
another machine. An iPhone asks for motion access the first time. Whenever
tilt can't steer, the menu says why and the thumb stick steers instead.

## Layout

```
src/
  main.js              renderer, bloom, menus, main loop, debug hooks
  levels/*.js          level data: route segments (or a closed loop), zones and
                       landforms, time of day, traffic rules, rivals
  levels/seaside/      Level 5's survey data (generated by tools/seaside/build.py)
                       and its loader
  track/Track.js       samples the route every metre; frames, racing line, AI speed
                       profile; loops wrap around; surveyed heights, camber, walls
                       and run-off for a circuit
  world/Terrain.js     height field sculpted around the road; landforms: mountain,
                       valley, city, coast, beach, harbor, streets, canyon, desert,
                       playa, raceway (surveyed ground)
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
  world/Raceway.js     Level 5 circuit: kerbs, walls, tyre walls, catch fencing,
                       start gantry and lights, grid, pit lane, buildings,
                       grandstands, bridges, lake, oaks
  world/raceway/       Level 5 textures: kerbs, tyres, fence, crowd, banners
  vehicles/            car models, player physics, rival AI, traffic, collisions,
                       the police driver
  game/                race flow, camera, HUD, effects, audio, input, touch controls,
                       tilt steering, gamepads (Gamepad.js: maps, remapping and
                       rumble; MenuNav.js: menus; PadSetup.js: the Controller screen)
  game/Pursuit.js      Hot Pursuit: heat, line of sight, busts, units, roadblocks
                       (no three.js, so it's tested in Node); PursuitView.js wires
                       it into a race
  game/audio/          the soundtrack: step sequencer and synth instruments, the songs,
                       and the drum kit and crash sounds pre-rendered at start-up;
                       the police radio's lines and the loader for their recordings
audio/radio/           the police radio voice's recorded lines (tools/radio-voice/)
vendor/three/          three.js r180 (the build plus the few add-ons used)
tools/                 check-track.js, car-test.html, audio-test.html, serve.py,
                       og-image.mjs, radio-voice.html and radio-voice/ (the police
                       radio voice: design, record, audition), dj-voice.html and
                       dj-voice/ (sample DJ chatter, the same way), seaside/build.py
                       (Level 5's data from OpenStreetMap and USGS)
test/unit/, test/e2e/  the tests (see Tests below)
music.html             the soundtrack player
```

## Dev hooks (URL parameters)

- `?s=2350&h=8&back=16&lat=0&yaw=0&pitch=-0.1&t=0.5`: a fly-along debug camera
  at a point on the track, with an optional fixed time of day `t` (0 to 1).
- `?level=sierra|coast|streets|desert|seaside|cruise`: choose the level.
- `?autostart=sports|muscle|super|rally|electric`: skip the menu and start a race.
- `?autodrive=1`: a simple autopilot drives the player car (for testing).
- `?timescale=2`: run the simulation faster.
- `?stats=1`: frame time, draw calls and triangle count.
- `?touch=1` / `?touch=0`: force the on-screen touch controls on or off (they
  appear automatically on touch screens).
- `?pursuit=1` / `?pursuit=0`: force Hot Pursuit on or off (sprint levels).
  `?heat=1..5` sets the starting heat, and `?cops=N` caps the number of police
  units (`?cops=0` gives the mode with no police, for testing the HUD).
  `window.__pursuit` is the live pursuit (`src/game/Pursuit.js`).

To check a route after editing its level file, run `node tools/check-track.js <level>`
(or `npm run check-track`, which checks Level 1). It
reports each zone's length and height, the tightest corner, the steepest
grade, and any place where two parts of the road overlap.

The link-preview picture, `og-image.png`, is a shot from the game (Hot Pursuit
on Interstate 9) with the logo on it. `node tools/og-image.mjs` renders it
again, in headless Chrome with no server, like the tests.

## Music player

`music.html` (the **Music player** link on the menu) plays the soundtrack
through the game's own music bus, so it sounds as it does in a race. It shows
each song's sections and parts; tap a section or the timeline to jump there,
and tap a part to solo it. It also has a repeat mode and an output meter
(level and spectrum), and on a keyboard Space, ← →, N and P work.

## Tests

```sh
npm install        # once: puppeteer-core, which drives the Chrome you have installed
npm test           # both suites
npm run test:unit  # plain Node, about 2 s
npm run test:e2e   # the browser, a few minutes
```

The unit tests in `test/unit/` run the code that doesn't need a browser:
every level's route, the car physics (including the car claims above), the
rivals driving each route, passing slower cars and keeping off a car
alongside, traffic and collisions (including getting off a wall with a rival
jammed against you), input, gamepads (bindings, remapping, rumble), tilt steering and the touch stick and pedal slider, the timer, the soundtrack's songs and synth
patches, Hot Pursuit (line of sight, heat, busts and escapes, and each
sprint level raced with the police on), and Seaside Raceway's survey data
(the lap, the climb and the Corkscrew, the walls, the run-off and the ground
under it).

The browser tests in `test/e2e/` play the real game in headless Chrome on the
GPU, both as a desktop and as a phone (touch and tilt, in landscape and
portrait; the tilt comes through Chrome's own sensor emulation), and with a
fake gamepad in place of `navigator.getGamepads` (menus, remapping, rumble). They
tap and click the way a player does, with the browser's real rules for when
audio and fullscreen may start. The working tree is served through request
interception, so the tests need no server and no port. Set `MR_BASE_URL` to test a running copy
instead (for example the GitHub Pages build), `CHROME_PATH` if Chrome isn't at
`/usr/bin/google-chrome`, and `MR_HEADFUL=1` to watch.

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
