"""Record the police radio lines in the designed voice (Qwen3-TTS voice clone).

Lists every line the game can say (lines.mjs, from
src/game/audio/radioLines.js), clones the voice chosen in voices.json from
its reference clip (voices/<name>.flac, made by design.py) with the
Qwen3-TTS Base model, and writes audio/radio/<clip>.mp3 (<clip>.2.mp3 for a
second take): trimmed, levelled, 16 kHz mono, since the game's radio bus
keeps only 300-3000 Hz anyway. audio/radio/index.json tells the game which
clips exist. Clips already on disk are kept, so after changing a line only
the new words are recorded; a different voice re-records everything.

    python tools/radio-voice/render.py                 # record what's missing
    python tools/radio-voice/render.py --only unit-2   # re-record ids containing this
    python tools/radio-voice/render.py --force         # re-record everything

Needs a Python with qwen-tts (https://github.com/QwenLM/Qwen3-TTS), a CUDA
GPU (about 5 GB free), node and ffmpeg.
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
OUT = ROOT / 'audio' / 'radio'
CLONE_MODEL = 'Qwen/Qwen3-TTS-12Hz-1.7B-Base'
BATCH = 2  # lines at a time: decoding repeats the reference clip for each, and 5 GB of GPU is tight
RATE = 16000


def take_file(clip_id, take):
    return OUT / (f'{clip_id}.mp3' if take == 1 else f'{clip_id}.{take}.mp3')


def finish(wav, sr):
    """Trim the silence, level the speech and cap the peaks."""
    wav = np.asarray(wav, dtype=np.float32)
    _, (a, b) = librosa.effects.trim(wav, top_db=38, frame_length=512, hop_length=128)
    wav = np.concatenate([np.zeros(int(sr * 0.03), np.float32), wav[a:b], np.zeros(int(sr * 0.05), np.float32)])
    frames = librosa.util.frame(np.pad(wav, (0, 512)), frame_length=512, hop_length=256)
    rms = np.sqrt((frames ** 2).mean(axis=0))
    voiced = rms[rms > rms.max() * 0.1]
    level = np.sqrt((voiced ** 2).mean()) if voiced.size else 1e-3
    wav *= 10 ** (-18 / 20) / level  # speech at -18 dBFS RMS
    peak = np.abs(wav).max()
    if peak > 0.89:
        wav *= 0.89 / peak  # peaks at -1 dBFS
    return wav


def encode(wav, sr, path):
    subprocess.run(
        ['ffmpeg', '-v', 'error', '-y', '-f', 'f32le', '-ar', str(sr), '-ac', '1', '-i', '-',
         '-ar', str(RATE), '-c:a', 'libmp3lame', '-b:a', '32k', '-map_metadata', '-1', str(path)],
        input=wav.tobytes(), check=True)


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument('--only', action='append', default=[], help='re-record clips whose id contains this (repeatable)')
    ap.add_argument('--force', action='store_true', help='re-record every clip')
    ap.add_argument('--seed', type=int, default=11, help='change it to get different takes')
    args = ap.parse_args()

    cfg = json.loads((HERE / 'voices.json').read_text())
    voice = cfg['voice']
    clips = json.loads(subprocess.run(['node', str(HERE / 'lines.mjs')], capture_output=True, check=True, text=True).stdout)
    OUT.mkdir(parents=True, exist_ok=True)
    index_path = OUT / 'index.json'
    old = json.loads(index_path.read_text()) if index_path.exists() else {}
    redo_all = args.force or bool(old) and old.get('voice') != voice

    jobs = []  # (clip, take)
    for c in clips:
        for take in range(1, c['takes'] + 1):
            wanted = redo_all or any(o in c['id'] for o in args.only) or not take_file(c['id'], take).exists()
            if wanted:
                jobs.append((c, take))
    print(f'{len(clips)} clips, {len(jobs)} takes to record in the "{voice}" voice', flush=True)

    if jobs:
        model = Qwen3TTSModel.from_pretrained(CLONE_MODEL, device_map='cuda:0', dtype=torch.bfloat16)
        prompt = model.create_voice_clone_prompt(ref_audio=str(HERE / 'voices' / f'{voice}.flac'), ref_text=cfg['ref_text'])
        t0 = time.time()
        for i in range(0, len(jobs), BATCH):
            batch = jobs[i:i + BATCH]
            torch.manual_seed(args.seed + i)
            wavs, sr = model.generate_voice_clone(
                text=[c['text'] for c, _ in batch], language=['English'] * len(batch),
                voice_clone_prompt=prompt * len(batch))
            for (c, take), wav in zip(batch, wavs):
                out = finish(wav, sr)
                encode(out, sr, take_file(c['id'], take))
                print(f'  {take_file(c["id"], take).name}: {len(out) / sr:.1f} s', flush=True)
            print(f'{min(i + BATCH, len(jobs))}/{len(jobs)} in {time.time() - t0:.0f} s', flush=True)

    # Keep only what the script can still say, and tell the game what's there.
    keep = {take_file(c['id'], t).name for c in clips for t in range(1, c['takes'] + 1)}
    for f in OUT.glob('*.mp3'):
        if f.name not in keep:
            f.unlink()
            print(f'  removed {f.name} (no longer in the script)')
    have = {}
    for c in clips:
        n = 0
        while n < c['takes'] and take_file(c['id'], n + 1).exists():
            n += 1
        if n:
            have[c['id']] = n
    index_path.write_text(json.dumps({'voice': voice, 'clips': have}, indent=0, sort_keys=True) + '\n')
    print(f'{index_path.relative_to(ROOT)}: {len(have)} of {len(clips)} clips')


if __name__ == '__main__':
    main()
