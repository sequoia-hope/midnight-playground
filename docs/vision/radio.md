# Radio: stations and DJs in the game

Status: **draft design** for vision ROADMAP M16, with a small first
version that can come much earlier (section 6). The DJ side quest
(`tools/dj-voice/`, `tools/dj-voice.html`) is the test of whether the talk
lands; this is how it gets into the game once it does.

The JS game is frozen and the Rust port keeps exact parity until cutover,
so radio lands in the Rust game after cutover (port ROADMAP M9). It does
not need the new sound engine of `sound.md`: it plays recorded clips over
the existing music, like the police radio does.

## 1. What the player gets

- **Stations instead of a playlist.** A button (and a key, and a pad
  binding) tunes the radio: The Tide, Ridgeline Radio, off.
- **You hear the dial move** (section 2.1): pressing the button doesn't cut
  to the next station; it sounds like the driver reaching over and turning
  the tuner, through static and whistles and a scrap of somewhere else,
  until the station locks in.
- **Each station has its own music and its own DJ.** At first the stations
  share out the existing seven songs by mood:
  - **The Tide** (Marisol): Seabright Dawn, Interstate Nights, Midnight
    Run, Mirage Highway;
  - **Ridgeline Radio** (Kit): Neon Rush, Afterburner, Chrome Heart.
  Later each station gets its own genre from the new music system
  (`sound.md` section 3.2: house on The Tide, techno and drum and bass on
  Ridgeline).
- **The DJ talks between songs, rarely,** and now and then when something
  worth mentioning happens. Silence is the default.
- The level still picks a starting station, as `LEVEL_TRACK` picks a song
  today.

## 2. The rules for talking

Sparse and reactive (WORLD 1.1). A small **director** in the client
decides when a DJ speaks:

- **When:** at song changes (the main slot), at the start of a drive, and
  after notable events. Never during a countdown, a final lap, a close
  finish, or while the police radio is talking.
- **How much:** a budget, such as at most one break every three or four
  minutes, and most song changes have none. Tunable, with a "DJ talk:
  off, rare, normal" setting.
- **What:** a break is a station ID plus at most one or two lines, chosen
  by context:
  - the level or region (`places`, Alice's on the Skyline levels);
  - the time of day and the weather once those exist (`weather`,
    `morning` only at dawn);
  - the mode (`scene` lines in pursuits and free drives);
  - what just happened (`player` lines, section 3).
- **No repeats:** each line has a cooldown, and a history kept with the
  player's records so a line isn't heard twice in a session, and rarely in
  a week.
- **Mixing:** the music ducks under the voice by a few dB, with a short
  ramp, then comes back. The voice goes through the music bus, so the
  music volume controls it, with its own trim.

### 2.1 The tuner

The owner's idea: the sense that the driver turned the dial. Games like
GTA cut stations with a short burst of static (from memory; not
confirmed). We go further, and generate it, like the game's other effects
(`audio/samples.js`), so it is never the same twice:

- **The sweep, about half a second to a second:** band-passed noise whose
  centre glides with the "dial", heterodyne whistles that rise and fall
  as it passes carriers, and a flicker of a station that isn't one of
  ours (a word, a bar of music) fading through.
- **The lock:** the static drops away while the station fades in, first
  narrow and a little detuned (thin, as if not quite on frequency), then
  opening to full width within a few hundred milliseconds.
- **Off:** a click, and the hiss falling away.
- **Ridgeline is a pirate:** its signal is weaker. Static creeps in on the
  far side of the ridge, in tunnels, and in deep valleys, and it drops out
  for a moment under bridges. The Tide is clean near the coast and fades
  inland.
- **Stations are live:** each station keeps its own clock, so tuning away
  and back finds it further along, mid-song, as a real station would, not
  restarted.
- On screen, the station's name and frequency for a moment; later, with
  the cockpit camera, the dial on the dash moves.

## 3. Reacting to the player

The "depth from memory" lines are templates with slots: a car, a colour, a
road, a rival, a margin. Two ways to voice them:

1. **Pre-render every combination** that can happen. With the game's
   cars, colours and roads that is tens to a few hundred clips per
   template: cheap to record offline with the same tools, and every clip
   sounds natural. **This is the first choice.**
2. Stitch recorded fragments ("Somebody in a" + "black 3000GT" + "just
   took the" + "Skyline" + "record") only if the combinations grow too
   many. Prosody suffers, so it is a fallback.

The facts come from what the game already tracks: records per level, best
laps, the car and its colour, pursuit escapes, rivals beaten. Events that
can trigger a line: a new record, a pursuit escaped, a rival beaten twice
in a row, a long night drive in the cruise mode.

## 4. Data and files

- **The script** stays in `tools/dj-voice/lines.json`, gaining per-line
  tags: `when` (song change, drive start, event name), `levels`,
  `time` and `weather` conditions, `cooldown`.
- **The clips** live in `audio/dj/` as now, with `index.json`. A station's
  clips are fetched when the station is first tuned, not up front, as the
  police radio fetches its clips when a pursuit starts.
- Recording, auditioning and rewriting stay in the side quest's tools.

## 5. Code (Rust, after cutover)

- `mr_audio::dj`: beside `mr_audio::radio` (the police radio), reusing its
  fetching and decoding. Plays clips, ducks the music, generates the
  tuner sweep and the reception noise.
- `mr_audio::music`: playlists per station instead of one playlist; a
  hook at song changes for the director.
- **The director** lives in the client (`mr_game`). It reads `SimEvent`s
  and the player's records. It is presentation, not simulation: its random
  choices use its own generator, never the simulation's streams, and in
  multiplayer each player hears their own radio.
- **UI:** the tune button, the station name on screen for a moment when
  it changes, the DJ-talk setting.

## 6. Steps

| | What | Gate |
|---|---|---|
| **R0. Side quest** | Lines recorded, auditioned, rewritten from the owner's 👍/👎 | Enough lines the owner likes to fill two stations |
| **R1. Radio v0** | Two stations over the existing songs, live (own clocks); station IDs and general lines at song changes; ducking; the tune button with the generated tuner sweep; the DJ-talk setting | The owner drives a whole cruise session with the radio on and doesn't turn the DJ off |
| **R2. Context** | Tags; lines chosen by level, mode and event; cooldowns and history; reception (Ridgeline's static on the far side of the ridge, tunnels) | Lines feel like they belong where they're heard |
| **R3. Memory** | Player templates pre-rendered for every combination; records and rivals feed them | The owner hears a line about their own run and it lands |
| **R4. Real stations** | Each station's own music from the new music system (`sound.md` S4) | vision ROADMAP M16's gate |

R1 needs nothing but cutover, so it can come right after it, alongside
multiplayer (M10), well before the rest of M16.
