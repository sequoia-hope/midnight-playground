# DJ chatter: sample radio talk

A side quest from vision ROADMAP M16 (radio): two DJs, about 140 short
lines across 15 topics, recorded so the owner can hear whether radio talk
lands before any of it goes in the game. Nothing here is wired into the
game.

| File | What |
|---|---|
| `lines.json` | The script: the two DJs, their topics, every line |
| `voices.json` | Three candidate voices per DJ (descriptions), and which one is chosen |
| `design.py` | Makes `voices/<candidate>.flac` from each description |
| `render.py` | Records every line in each DJ's chosen voice into `audio/dj/` |
| `../dj-voice.html` | The listening page: play, compare takes, car-stereo mode, 👍/👎 notes |

## For the agent running it

Same setup as `tools/radio-voice/`: a Python with the `qwen-tts` package
(https://github.com/QwenLM/Qwen3-TTS), a CUDA GPU with about 5 GB free, and
ffmpeg. Run from the repo root:

```
python tools/dj-voice/design.py     # 6 candidate voices, about a minute
python tools/dj-voice/render.py     # ~140 lines x 2 takes, into audio/dj/
```

Then open `tools/dj-voice.html` through the project's server
(`proj up midnight-racer`, path `/tools/dj-voice.html`) and tell the owner
it's ready.

Both scripts only do what's missing, so they are safe to re-run after an
interruption. `render.py --only <id-part>` re-records matching clips,
`--seed N` gives different takes, `--takes 3` adds a third take. Changing a
DJ's `"chosen"` voice, or editing a line, re-records just what changed.

If the long "good morning" lines (`marisol-morning-1`, `kit-morning-1`)
come out garbled, they are already split into parts at their blank lines;
split them further in `lines.json` and run `render.py` again.

Commit `audio/dj/` and `tools/dj-voice/voices/` with the scripts when the
owner is happy, as `audio/radio/` is; the clips are small (24 kHz mono, 64
kbit/s MP3).

## For the owner

Listen on the page, try the **Car stereo** mode and **A random radio
break**, mark lines 👍 or 👎, then **Copy my notes** and paste them to
Claude to rewrite what doesn't land. The `player` topic is the "depth from
memory" test (vision WORLD 1.1): those lines are templates the game would
fill from what the player actually did, recorded here with one example
each.
