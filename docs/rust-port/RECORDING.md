# Run recordings

A run recording is a log of one session of the Rust client, written for an
agent to examine afterwards: what the client did, how its frames went, and
every race in enough detail to replay it headless, tick for tick. The owner
records a run where something looked wrong; the agent reads the file. The
choices behind it are DECISIONS D1021 and D1022; the code is
`crates/mp_game/src/recording.rs` and `crates/mp_sim/src/replay.rs`.

## Making one

Natively:

```
cargo run --release -p mp_game -- --record                  # the menu, as the player starts it
cargo run --release -p mp_game -- --record --level coast    # straight into a race
cargo run --release -p mp_game -- --query "record=/some/dir"   # elsewhere (a dir, or a .jsonl file)
```

`--record` is `record=1`. The file is
`recordings/<UTC date>-<time>Z-<level>.jsonl` under the repository root
(`recordings/` is ignored by git); its path is printed when the run starts
and again, with its size, when it ends. The level in the name is the one
the run started on. Lines are flushed every second, so a crash loses at
most the last second; a clean exit (closing the window, Ctrl+Q, Ctrl+C)
writes an `end` line.

On the web, `?record=1` keeps the lines in the page's memory:
`__mp.recording()` returns them as text and `__mp.saveRecording()` saves
them as a download. The web has no `log` lines (the log layer is native).

The debug overlay (`--debug`, `debug=1`, F3; DECISIONS D1020) is
independent; with both on, the recording notes when the overlay is shown.

## The format

JSON lines, one object a line. Every line starts with `t`, seconds since
the recording started (the same clock for every line), and `type`. A line
of a type below is written when the thing it reports changes, unless it
says otherwise. Floats are rounded for size; ms are milliseconds.

| `type` | When | Fields |
|---|---|---|
| `start` | first line | `version` (1), `banner`, `build` (`debug`/`release`), `platform`, `date_utc`, `commit` (natively, the checkout's `git rev-parse HEAD`), `file`, `args`, `query` (the options as `[key, value]` pairs), `level`, `hq`, `race_on`, `tick_hz` (120), `chunk` (ticks per `ticks` line), `slow_ms` |
| `adapter` | once the GPU is known | `name`, `backend`, `device_type`, `driver`, `driver_info` |
| `status` | the loading status changes | `state` (`waiting`, `building`, `warming`, `running`, `failed`), `ready`, `frame`, `scenes`, `error`, `counts` |
| `mode` | the client's mode or screen changes | `mode`: `<mode>/<screen>`, mode as `window.__game.mode` (`loading`, `menu`, `race`, `paused`, `results`), screen as the UI's (`loading`, `menu`, `pause`, `results`, `padsetup`, `none`); a run without the race UI (the viewer) has the status `state` instead |
| `settings` | at start and on every change | the menu's settings: `music`, `sfx`, `mph`, `hq`, `autogas`, `steering`, `tilt_sens`, `pedals`, `fullscreen`, `car`, `level`, `track`, `flash`, `rumble` |
| `frames` | once a second | `n` frames, `fps`, the frame interval's `mean`, `min`, `p99`, `max`; `main_mean`/`main_max` (the main world's CPU time, First to Last), `render_mean`/`render_max` (the render world's `Render` schedule, the wait for the display included, so about a frame at vsync); `slow` frames, sim `ticks` run, `mode`, `pipelines_waiting`, `late_frames` (frames with a pipeline compiling after the warm-up, so far) |
| `slow_frame` | every frame whose interval or main-world time is over `slow_ms` (50) | `frame` (number), `ms` (the interval that ended as this frame began), `main_ms` (this frame's own main-world work: a long one shows again as the next frame's `ms`), `render_ms`, `mode`, `pipelines_waiting` |
| `race_start` | a race is built, or restarted | `race` (the client's race number), `level`, `car`, `seed`, `pursuit`, `heat`, `cops`, `flash`, `pursuit_on`, `cruise`, `laps`, `autodrive`, `touch`, `timescale`, `rivals`, `traffic`, `tick` |
| `ticks` | every 120 ticks of a race (at a frame's end, so a few more), and before a mode change or the race's end | `race`, `from` (first tick), `tick` (last tick), `hash` (`mp_sim::race::hash` of the state after `tick`, hex; missing on the last line of a race that was replaced), `in` (the inputs, below), `race_state`, `race_time`, `p` (the player's car after `tick`: `s`, `lat`, `x`, `y`, `z`, `yaw`, `kmh`, `gear`, `rpm`, `nitro`, `nitro_on`, `drift`, `on_ground`, `lap`, `alive`), `ev` (the simulation's events in those ticks, `[tick, "Debug text"]`, the tick being the frame's last) |
| `gap` | the inputs did not line up with the ticks (should not happen) | `race`, `ticks`, `inputs`, `tick` |
| `race_mode` | pause, resume, results | `race`, `mode` (`race`, `paused`, `results`), `tick` |
| `race_end` | the results come up | `race`, `tick`, `time`, `finish_time`, `lap_times`, `results` (`place`, `name`, `player`, `time`, `estimated`), `cruise` (Night City Cruise's results, Debug text), `pursuit` (Hot Pursuit's stats, Debug text) |
| `race_stop` | the race is taken away (to the menu) or replaced | `race`, `tick` |
| `window` | size, scale, mode or focus changes (checked four times a second) | `width`, `height` (physical px), `scale`, `mode`, `focused` |
| `focus` | the window gains or loses the focus | `focused` |
| `gamepad` | natively, a pad connects or disconnects | `gamepad`, `connection` (Debug text: name, vendor and product) |
| `pads` | the pads' state changes | `connected`, `active` (`id`, `index`, `mapping`) |
| `audio` | the sound's state changes (checked four times a second) | `ready`, `context` (`running`, `suspended`, `closed`), `music_on`, `paused`, `environment`, `track`, `volumes` (master, sfx, music) |
| `overlay` | the debug overlay is shown or hidden | `on` |
| `log` | natively, a log line: every warning and error, and the client's own (`mp_*`) info lines | `level`, `target`, `msg` (at most 1000 a frame) |
| `end` | last line, on a clean exit | `why`, `seconds`, `frames`, `slow_frames`, `races`, `lines` |

### The inputs

`in` is every tick's `InputFrame` (`mp_sim::input`), as the simulation took
it, in order from `from` to `tick`: `steer,throttle,brake,flags` separated
by spaces, a run of equal frames written once with `*n`
(`0,255,0,4*30`). `steer` is −32767..32767 (left negative), `throttle` and
`brake` 0..255, `flags` the sum of handbrake 1, nitro 2, analog 4 (a pad or
the touch stick), reset 8. This is what the keyboard, the pads, the touch
controls, tilt and the autopilot (`autodrive=1`) came to, after the input
layer's ramps; which device made it is not recorded, but `analog` says
whether it was a stick, and the `pads` and `settings` lines say what was in
use. A paused race runs no ticks, so its inputs simply stop and resume.

## Reading one (for agents)

The file can be long (about 1.5 KB a second of racing, mostly `in`; a few
hundred bytes a second otherwise), so filter by type rather than reading it
whole:

```
grep -c . run.jsonl                                   # lines
grep '"type":"race_' run.jsonl                        # each race's start, pauses, results, stop
grep '"type":"slow_frame"' run.jsonl                  # every hitch over 50 ms, with what was going on
grep '"type":"log"' run.jsonl | grep -v '"info"'      # warnings and errors
python3 -c 'import json,sys
for l in open(sys.argv[1]):
    d = json.loads(l)
    if d["type"] == "frames": print(d["t"], d["fps"], d["p99"], d["max"], d["main_max"], d["mode"])' run.jsonl
```

To see a frame problem, line `slow_frame` and `frames` up with the `mode`,
`status`, `log` and `race_*` lines around the same `t`. A stutter that the
simulation did not cause shows as frames over 50 ms with `ticks` normal;
a pipeline compiled late shows `pipelines_waiting` > 0 and a `log` line
naming it.

To examine a race, replay it: the simulation is deterministic and the
recording has its seed, options and every tick's input.

```
cargo run --release -p mp_sim --bin mp-sim -- replay run.jsonl                 # every race: checkpoints, results
cargo run --release -p mp_sim --bin mp-sim -- replay run.jsonl --race 2 --trace race2.trace
cargo run --release -p mp_sim --bin mp-sim -- replay run.jsonl --race 2 --state-at 6000
```

`replay` steps a new state with the recorded inputs and compares the hash
at every `ticks` line's `tick`; it prints `N checkpoints match` (or the
first tick that differs, and fails) and the results. `--trace` writes the
full per-tick trace (`parity/trace-format.md`) of one race (the last one
replayed; pick it with `--race`), the format the parity tools read;
`--state-at` prints the whole `SimState` after that tick. Seaside needs its
survey (`--survey`, default `assets/seaside/survey.bin`, run from the
repository root). A recorded race replays only on the code it was recorded
with (the `start` line's `commit`; uncommitted changes are not recorded): a change
to the simulation since then shows as a checkpoint that differs.

What a replay cannot reproduce: state the web test bridge changed from
outside (`__mp.stage`, the e2e suites' hooks), which a player never does.
The camera, the effects and the sound are not in the simulation; their
inputs are (the `p` and `ev` fields say what they reacted to).
