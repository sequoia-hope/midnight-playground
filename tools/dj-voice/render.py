"""Record the DJ chatter in each DJ's chosen voice (Qwen3-TTS voice clone).

Reads every line in lines.json, clones each DJ's chosen voice from
voices.json (voices/<candidate>.flac, made by design.py) with the Qwen3-TTS
Base model, and writes audio/dj/<id>.mp3 (<id>.2.mp3 for the second take
and so on): trimmed, levelled, 24 kHz mono. Unlike the police radio this is
full range, since the game will play it as a car stereo, not a handset.

A clip's id is <dj>-<topic>-<n>. A text with blank lines in it is recorded
part by part and joined with a short pause, so the long ones hold together.
audio/dj/index.json records which voice and which text each clip was made
from, so only what changed is recorded again: a new or edited line, or a DJ
whose chosen voice changed. Clips no longer in the script are removed.

    python tools/dj-voice/render.py                    # record what's missing or changed
    python tools/dj-voice/render.py --only kit-morning # re-record ids containing this
    python tools/dj-voice/render.py --takes 3          # three takes of every line
    python tools/dj-voice/render.py --force            # re-record everything

Needs a Python with qwen-tts (https://github.com/QwenLM/Qwen3-TTS), a CUDA
GPU (about 5 GB free) and ffmpeg.
"""
import argparse
import json
import os
import subprocess
import time
from pathlib import Path

os.environ.setdefault('PYTORCH_ALLOC_CONF', 'expandable_segments:True')
import librosa
import numpy as np
import torch
from qwen_tts import Qwen3TTSModel

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
OUT = ROOT / 'audio' / 'dj'
CLONE_MODEL = 'Qwen/Qwen3-TTS-12Hz-1.7B-Base'
BATCH = 2  # parts at a time: decoding repeats the reference clip for each, and 5 GB of GPU is tight
RATE = 24000
PAUSE = 0.45  # seconds between the parts of a long line


def script():
    """Every clip as {id, dj, topic, text, parts}, in lines.json order."""
    data = json.loads((HERE / 'lines.json').read_text())
    clips = []
    for dj, d in data['djs'].items():
        for topic in data['topics']:
            for n, line in enumerate(d['lines'].get(topic, []), 1):
                text = line['text'] if isinstance(line, dict) else line
                parts = [p.strip() for p in text.split('\n\n') if p.strip()]
                clips.append({'id': f'{dj}-{topic}-{n}', 'dj': dj, 'topic': topic, 'text': text, 'parts': parts})
    return clips


def take_file(clip_id, take):
    return OUT / (f'{clip_id}.mp3' if take == 1 else f'{clip_id}.{take}.mp3')


def trim(wav, sr):
    wav = np.asarray(wav, dtype=np.float32)
    _, (a, b) = librosa.effects.trim(wav, top_db=38, frame_length=512, hop_length=128)
    return wav[a:b]


def level(wav, sr):
    """Pad, set the speech to -18 dBFS RMS and cap the peaks at -1 dBFS."""
    wav = np.concatenate([np.zeros(int(sr * 0.03), np.float32), wav, np.zeros(int(sr * 0.08), np.float32)])
    frames = librosa.util.frame(np.pad(wav, (0, 512)), frame_length=512, hop_length=256)
    rms = np.sqrt((frames ** 2).mean(axis=0))
    voiced = rms[rms > rms.max() * 0.1]
    lvl = np.sqrt((voiced ** 2).mean()) if voiced.size else 1e-3
    wav = wav * (10 ** (-18 / 20) / lvl)
    peak = np.abs(wav).max()
    if peak > 0.89:
        wav *= 0.89 / peak
    return wav.astype(np.float32)


def encode(wav, sr, path):
    subprocess.run(
        ['ffmpeg', '-v', 'error', '-y', '-f', 'f32le', '-ar', str(sr), '-ac', '1', '-i', '-',
         '-ar', str(RATE), '-c:a', 'libmp3lame', '-b:a', '64k', '-map_metadata', '-1', str(path)],
        input=wav.tobytes(), check=True)


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument('--only', action='append', default=[], help='re-record clips whose id contains this (repeatable)')
    ap.add_argument('--force', action='store_true', help='re-record every clip')
    ap.add_argument('--takes', type=int, default=2, help='takes per line (default 2)')
    ap.add_argument('--seed', type=int, default=11, help='change it to get different takes')
    args = ap.parse_args()

    cfg = json.loads((HERE / 'voices.json').read_text())
    chosen = {dj: d['chosen'] for dj, d in cfg['djs'].items()}
    for dj, cand in chosen.items():
        if not (HERE / 'voices' / f'{cand}.flac').exists():
            raise SystemExit(f'{dj}: voices/{cand}.flac is missing; run design.py first')
    clips = script()
    OUT.mkdir(parents=True, exist_ok=True)
    index_path = OUT / 'index.json'
    old = json.loads(index_path.read_text()) if index_path.exists() else {'clips': {}}

    def stale(c):
        o = old['clips'].get(c['id'])
        return not o or o.get('text') != c['text'] or o.get('voice') != chosen[c['dj']]

    jobs = []  # (clip, take)
    for c in clips:
        redo = args.force or any(o in c['id'] for o in args.only) or stale(c)
        for take in range(1, args.takes + 1):
            if redo or not take_file(c['id'], take).exists():
                jobs.append((c, take))
    print(f'{len(clips)} lines, {len(jobs)} takes to record', flush=True)

    made = {}  # id -> takes recorded in this run (for the index)
    if jobs:
        model = Qwen3TTSModel.from_pretrained(CLONE_MODEL, device_map='cuda:0', dtype=torch.bfloat16)
        t0 = time.time()
        done = 0
        for dj in chosen:
            mine = [j for j in jobs if j[0]['dj'] == dj]
            if not mine:
                continue
            prompt = model.create_voice_clone_prompt(
                ref_audio=str(HERE / 'voices' / f'{chosen[dj]}.flac'), ref_text=cfg['djs'][dj]['ref_text'])
            print(f'{dj}: {len(mine)} takes in the "{chosen[dj]}" voice', flush=True)
            # Every part of every take is one TTS job; a take is the joined parts.
            parts = [(ji, pi, p) for ji, (c, _) in enumerate(mine) for pi, p in enumerate(c['parts'])]
            audio = {}
            sr = None
            for i in range(0, len(parts), BATCH):
                batch = parts[i:i + BATCH]
                torch.manual_seed(args.seed + i)
                wavs, sr = model.generate_voice_clone(
                    text=[p for _, _, p in batch], language=['English'] * len(batch),
                    voice_clone_prompt=prompt * len(batch))
                for (ji, pi, _), wav in zip(batch, wavs):
                    audio[(ji, pi)] = trim(wav, sr)
                # Write each take as soon as all its parts exist.
                for ji, (c, take) in enumerate(mine):
                    n = len(c['parts'])
                    if (ji, n - 1) in audio and (ji, 0) in audio and all((ji, k) in audio for k in range(n)):
                        gap = np.zeros(int(sr * PAUSE), np.float32)
                        pieces = []
                        for k in range(n):
                            pieces += [audio.pop((ji, k))] + ([gap] if k < n - 1 else [])
                        wav = level(np.concatenate(pieces), sr)
                        encode(wav, sr, take_file(c['id'], take))
                        made.setdefault(c['id'], set()).add(take)
                        done += 1
                        print(f'  {take_file(c["id"], take).name}: {len(wav) / sr:.1f} s  ({done}/{len(jobs)}, {time.time() - t0:.0f} s)', flush=True)

    # Keep only what the script still says, and record what's there.
    keep = {take_file(c['id'], t).name for c in clips for t in range(1, args.takes + 1)}
    for c in clips:  # extra takes from an earlier, larger --takes stay
        t = args.takes + 1
        while take_file(c['id'], t).exists():
            keep.add(take_file(c['id'], t).name)
            t += 1
    ids = {c['id'] for c in clips}
    for f in OUT.glob('*.mp3'):
        if f.name not in keep or f.name.split('.')[0] not in ids:
            f.unlink()
            print(f'  removed {f.name} (no longer in the script)')
    index = {'voices': chosen, 'clips': {}}
    for c in clips:
        n = 0
        while take_file(c['id'], n + 1).exists():
            n += 1
        if not n:
            continue
        fresh = c['id'] in made
        o = old['clips'].get(c['id'], {})
        index['clips'][c['id']] = {
            'takes': n,
            'text': c['text'] if fresh else o.get('text', c['text']),
            'voice': chosen[c['dj']] if fresh else o.get('voice', chosen[c['dj']]),
        }
    index_path.write_text(json.dumps(index, indent=1, sort_keys=True) + '\n')
    print(f'{index_path.relative_to(ROOT)}: {len(index["clips"])} of {len(clips)} lines recorded')


if __name__ == '__main__':
    main()
