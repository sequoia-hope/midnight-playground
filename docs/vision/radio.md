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

- **Symmetric (the owner, 2026-10-09):** the first build was a sweep, a
  dial travelling one way: whistles sliding in pitch, our station bleeding
  in at the end and opening through a low-pass. The owner heard it as a
  whoosh "heading past you in one direction", lopsided, starting low and
  ending high. What it should be: tuning off one station and onto
  another, "the music fades to static, which then fades back to the other
  music", with static that has some character, so turns sound different.
- **Built (`mp_music::radio::Tuner`):** a turn is three parts: the
  station we leave fades into static (muffling as it goes: a low-pass
  closing from full width to a few hundred hertz), the static holds a
  moment (0.1 to 0.4 s), and the static fades into the new station (the
  same low-pass opening). The fade in is the fade out reversed: equal-power
  curves, the same length (0.2 to 0.4 s), nothing sliding in pitch. The
  player keeps the station being left playing through its fade (two
  engines take turns). Each turn draws its static's character afresh and
  holds it still: the hiss's colour and the receiver's bandwidth (AM-like:
  narrow, crackly, sometimes a steady whistle; FM-like: wide and smooth),
  a flutter as the signal comes and goes, and about half the time a
  far-off station under the noise, a voice or a chord, garbled. Tuned again
  mid-turn, it runs back from the same point, so nothing jumps.
  `cargo run --release -p mp_music --example tuner -- out.wav` renders
  eight turns to hear.
- **On and off:** on fades up from silence through the static to the
  station; off fades the station into the static and the static away.
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

- `mp_music::radio` (section 7): the stations, their schedules on the wall
  clock, the cue for a tune-in, and the tuner's static and lock. Pure: no
  clock of its own, the caller passes the time.
- `mp_audio`: a `Radio` node in the facade (the `mp_music` engine in an
  AudioWorklet, as the exhaust model), `GameAudio::set_station`, the DJ
  clips beside `mp_audio::radio` (the police radio), reusing its fetching
  and decoding, ducking the station while a clip plays.
- The seven arranged songs stay as the **Playlist** choice on
  `mp_audio::music`, unchanged.
- **The director** lives in the client (`mp_game`). It reads `SimEvent`s
  and the player's records. It is presentation, not simulation: its random
  choices use its own generator, never the simulation's streams, and in
  multiplayer each player hears their own radio.
- **UI:** the tune button, the station name on screen for a moment when
  it changes, the DJ-talk setting.

## 6. Steps

| | What | Gate |
|---|---|---|
| **R0. Side quest** | Lines recorded, auditioned, rewritten from the owner's 👍/👎 | Enough lines the owner likes to fill two stations |
| **R1. Radio v0** | Stations as persistent streams of the generated music (section 7), live on the wall clock; station IDs and general lines at song changes; ducking; the tune control with the generated tuner sweep; the DJ-talk setting | The owner drives a whole cruise session with the radio on and doesn't turn the DJ off |
| **R1.5. Favourites** | Each station's rotation (section 8): curated `(genre, seed)` songs woven into the schedule; the DJ's lines about them, pre-rendered per song; "keep" and "skip" on the Radio page to build the lists | Most of what a station plays has been heard by someone; a DJ names a song and means it |
| **R2. Context** | Tags; lines chosen by level, mode and event; cooldowns and history; reception (Ridgeline's static on the far side of the ridge, tunnels) | Lines feel like they belong where they're heard |
| **R3. Memory** | Player templates pre-rendered for every combination; records and rivals feed them | The owner hears a line about their own run and it lands |
| **R4. More stations** | Country and classical stations when their grammars exist (sound.md 3.5), each with its own DJ | vision ROADMAP M16's gate |

R1 was first planned over the seven arranged songs; the owner
(2026-10-08) made the generated music the foundation instead, so R1 and
the old R4 are one step.

## 7. Streams: stations that exist outside the player (2026-10-08)

The owner's direction: the generated music (sound.md 3) is the foundation
of a car radio with controls, channel selection, and persistent streams,
so that tuning to a station catches its song wherever it happens to be.

- **The clock.** A station's time is the wall clock (UTC) since its epoch,
  2026-01-01T00:00Z (`mp_music::radio::EPOCH`, the day the stations went
  on air). The schedule is a pure function of (station, time): every
  player, device and session hears the same song at the same moment; tune
  away and back and it has moved on; two players racing together hear the
  same station in step, each from their own clock (phones agree to well
  under a second). Nothing is stored and nothing is streamed.
- **Blocks.** Time is cut into 20-minute blocks (`BLOCK_SECS`). A block's
  songs come from a generator seeded by the station's seed and the block's
  index: genres drawn by the station's weights, a seed per song, grammar
  after grammar until the block is full. The last song is **fitted** to the
  block's end: whole 8-bar blocks of its groove are added or dropped, then
  its tempo nudged (under 4 %, inaudible on a song nobody has heard
  before) so it ends on the boundary. Songs follow each other without a
  gap, as on a station. A tune-in costs the block's four to six grammar
  runs, well under a millisecond in Rust; nothing walks the schedule from
  the epoch.
- **Catching a song mid-way.** The cue is the song, the 16th it is on and
  the fraction into that 16th; the engine starts on the next 16th with the
  section's state (the filter sweep, the automation) set as if it had
  played from the bar's start. Notes already sounding are missed, which a
  tune-in hides: it comes in through the tuner's static (2.1).
- **The node.** One AudioWorklet node per client (`mp_music` as its own
  small wasm, as `mp_exhaust`), driven by k-rate params: `station` (an
  index, −1 off), `wallDay` and `wallSec` (the wall time at the moment of
  tuning, in two parts because an `f32` cannot hold Unix seconds), `tune`
  (a serial, bumped to re-sync), `energy`. From then on the node runs the
  schedule on its own sample clock: at a song's end it computes the next
  from the station's time. The main thread computes the same schedule to
  show the title and to place the DJ's breaks. Natively the same code runs
  in `mp_audio`'s worklet processor, so both render alike.
- **The tuner.** Between stations: the station playing fades into
  static, the static holds, and it fades into the new station, the two
  fades mirror images (2.1). Generated in `mp_music::radio` (`Tuner`),
  seeded, never the same twice.
- **The DJ** talks from clips on the main thread's graph (as the police
  radio does), ducking the station: idents and song intros at the seams the
  schedule gives, chosen by the director in `mp_game` (section 2's rules).
- **Stations**, the first three (`mp_music::radio::STATIONS`):

  | Station | DJ | Music |
  |---|---|---|
  | The Tide, 88.1 | Marisol | house, UK garage, liquid drum & bass |
  | Ridgeline Radio | Kit | techno, psytrance, trance, eurobeat, roller drum & bass |
  | Radio Pacífico, 104.3 | Teo | chicha, with some house and garage |

  Country and classical get stations when their grammars exist (sound.md
  3.5.1, 3.5.2); a KPIG-like coast country station and a classical one.
- **Controls.** T and the pause screen's button ("Next station") step
  round the dial: the stations in order, then the Playlist (the old music,
  one stop on the dial; its own next track is the menu's picker), then
  the first station again; a toast shows the station and the song for a
  moment, and the pause screen's now-playing line names them; M toggles
  the music volume as before. The level picks a default station
  (`level_station`) as `LEVEL_TRACK` picks a song today, and the choice
  persists (`mr.station`, also a "Radio" select on the menu). Where the
  radio cannot run (no AudioWorklet: an insecure page) the playlist plays
  and T is the JS's next track.
- **Energy.** The race drives the station's `energy` (sound.md 3.4): a
  pursuit, the final lap and a close battle raise it, a standstill lowers
  it; the mix closes down and layers drop out below full. The Radio page
  has no race, so there energy grows with each song (half at the start,
  full by two thirds in) until a hand on the slider takes it over.
- **A station change cuts the DJ off** at once (the page as the game): a
  DJ does not follow the listener off their station.
- **The Radio page is a music app** (`tools/radio.html`, 2026-10-08, the
  owner: "make radio work like a music app", "keep playing when I close
  the phone"). Its graph ends in an `<audio>` element playing a
  `MediaStream` from the context, not the context's destination, so the
  browser treats the page as media: it keeps playing with the screen
  locked and the app in the background, the Silent switch does not mute
  it, and the lock screen and the notification shade carry the station
  and the song (Media Session: title, station and frequency, style, a
  drawn tile per station, the position in the song) with play, pause,
  next and previous (the dial). `navigator.audioSession.type =
  'playback'` is asked for before the context, as the game does. Pause is
  off; play comes back to the last station, live. The last station is
  remembered (`radio.station`), a manifest and icons make it installable
  from "Add to Home Screen" as "Radio", and what is on refreshes on a
  timer, which runs with the screen off where frames do not.
  `tools/parity/e2e/radio-page.test.mjs` checks it headless. The game's
  own radio does not do this: a game in the background is not a radio.

## 8. Favourites: the DJs know the records (2026-10-08)

The owner's direction. The chatter so far is about the world, which is
right, but a DJ should also know the music: each DJ has favourite songs,
real ones, and when they say "this is one of mine" the station plays
that song. And since a song here is only a `(genre, seed)` pair, a list
of favourites is also the way to audit the generated music: fully random
songs are made with no one paying attention, so their average is lower
than music a human picked, and the cheapest way to put the attention back
is at the end, by choosing, rather than at the start, by making the
generator intentional.

- **A song is `(genre, seed)`.** The grammar is deterministic, so the
  pair names the same song on every device forever (the goldens in
  `parity/golden/music/` already rest on this). A favourite is a pair
  someone listened to and kept, with the title the grammar gave it, the
  DJ it belongs to, and a note. If a grammar ever changes, its seeds mean
  different songs: version the grammar, and the goldens say when.
- **Two pools per station.** A block's songs are drawn from the station's
  **rotation** (its favourites, and its DJ's) most of the time, and from
  **discovery** (a fresh seed, as now) the rest. The rotation's share
  scales with the list: a station with four favourites plays them rarely,
  with forty most of the time, so a short list never repeats too much.
  The block stays a pure function of (station, time): the lists are
  compiled in, and the generator draws an index instead of a seed. The
  average goes up at once, and the random draws stop being the product
  and become the pipeline: every song that plays has an address someone
  can write down.
- **Rejects.** A seed heard and refused goes on the station's blocklist,
  so discovery never draws it again. That is the cheapest audit, a 👎.
  A reject with a reason ("the bass fights the kick", "the chorus never
  arrives") is a grammar bug, and fixing one improves every seed after
  it: the notes are the backlog for the grammars.
- **The DJ's lines about a song** are pre-rendered per favourite, as the
  `player` templates are pre-rendered per combination (section 3): a few
  templates filled with the title ("This one's mine. [title]. I've
  played it a hundred times and I'm not sorry."), and hand-written lines
  for the ones with a story. The director knows, at a song change,
  whether the next slot is a favourite and whose, and picks one of its
  lines instead of a generic intro. Favourites cross stations: Kit plays
  a house song and says it's Marisol's, which gives `banter` something
  true to chew on. The generic `music` lines stay for discovery.
- **Building the lists** (built 2026-10-08, D1155). The Radio page
  (`tools/radio.html`) plays the live schedule and shows what is on; by
  the now-playing line it has **Keep** and **Reject** with a note (K, J),
  which POST (station, dj, genre, seed, title, style, bpm, bars, verdict,
  note, wall, at) to `crates/mp_music/favourites.json`, the file beside
  the stations' Rust, which `tools/serve.py` writes on disk; a later
  verdict on the same pair replaces the earlier one, and the programme
  table marks every song the file knows. The controls show only where the
  file's GET says `X-Favourites: writable`, so on a clone served the
  registered way and not on GitHub Pages: the owner and the producer
  friends fill the lists by listening, and the verdicts travel with the
  repo as commits. Two affordances a real radio lacks: **Skip song** (S)
  runs the page's clock ahead of the stations' to the next song's start
  (the player stays a pure function of (station, time); the page says how
  far ahead it is, and **Live** (L) puts the clock back), and a song's
  **link** (`?station=<key>&wall=<its start on the station's clock>`, the
  `wall` saved with the verdict) replays the station at that moment, so a
  note can be checked. Crowd rating (a public stream where listeners rate
  songs) is the same page with a backend, later and not on the roadmap;
  the data shape is the same from day one, so nothing is thrown away.
  Either way the lists are filtered by hand at the least.
- **Data.** `crates/mp_music/src/radio.rs` gets the lists next to
  `STATIONS` (generated from `favourites.json` by the build, as the lab's
  tables are), a `rotation` weight per station, and the blocklist; the
  DJ's per-song lines go in `tools/dj-voice/lines.json` under a `songs`
  topic keyed by the pair.
