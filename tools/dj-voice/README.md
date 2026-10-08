# DJ chatter: sample radio talk

A side quest from vision ROADMAP M16 (radio): three DJs, about 210 short
lines across 15 topics, recorded so the owner can hear whether radio talk
lands before any of it goes in the game. Nothing here is wired into the
game.

| File | What |
|---|---|
| `lines.json` | The script: the three DJs, their topics, every line |
| `voices.json` | Three candidate voices per DJ (descriptions), and which one is chosen |
| `design.py` | Makes `voices/<candidate>.flac` from each description |
| `render.py` | Records every line in each DJ's chosen voice into `audio/dj/` |
| `../dj-voice.html` | The listening page: play, compare takes, car-stereo mode, 👍/👎 notes |

## For the agent running it

Same setup as `tools/radio-voice/`: a Python with the `qwen-tts` package
(https://github.com/QwenLM/Qwen3-TTS), a CUDA GPU with about 5 GB free, and
ffmpeg. Run from the repo root:

```
python tools/dj-voice/design.py     # 9 candidate voices, about a minute
python tools/dj-voice/render.py     # ~210 lines x 2 takes, into audio/dj/
```

Then open `tools/dj-voice.html` through the project's server
(`proj up midnight-racer`, path `/tools/dj-voice.html`) and tell the owner
it's ready.

Both scripts only do what's missing, so they are safe to re-run after an
interruption. `render.py --only <id-part>` re-records matching clips,
`--seed N` gives different takes, `--takes 3` adds a third take. Changing a
DJ's `"chosen"` voice, or editing a line, re-records just what changed.

If the long "good morning" lines (`marisol-morning-1`, `kit-morning-1`,
`teo-morning-1`)
come out garbled, they are already split into parts at their blank lines;
split them further in `lines.json` and run `render.py` again.

`audio/dj/` and `tools/dj-voice/voices/` are committed (2026-10-08) with
one take per line: the owner kept the second take of each render, as
`<id>.mp3`, and `index.json` says `takes: 1`. The clips are small (24 kHz
mono, 64 kbit/s MP3, 8.5 MB in all). Running `render.py` again adds the
missing second takes back; commit only what the owner picks.

## For the owner

Listen on the page, try the **Car stereo** mode and **A random radio
break**, mark lines 👍 or 👎, then **Copy my notes** and paste them to
Claude to rewrite what doesn't land. The `player` topic is the "depth from
memory" test (vision WORLD 1.1): those lines are templates the game would
fill from what the player actually did, recorded here with one example
each.

Teo (Radio Pacífico, added 2026-10-08) is bilingual and switches into
Spanish mid-sentence. Qwen3-TTS speaks Spanish, but a cloned voice
code-switching inside an English sentence is untested: his `ref_text`
mixes both on purpose, so listen to his three candidates for the Spanish
before recording his 71 lines, and tell the owner how it came out.
