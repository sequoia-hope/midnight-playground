"""Design the police radio voice with Qwen3-TTS VoiceDesign.

Each voice in voices.json is a plain-English description. This speaks
ref_text in each one and saves voices/<name>.flac: the reference clip that
render.py clones every radio line from (with the Qwen3-TTS Base model), so
the whole script comes out in one consistent voice. Listen to them in
tools/radio-voice.html, set "voice" in voices.json, then run render.py.

    python tools/radio-voice/design.py              # every voice
    python tools/radio-voice/design.py trooper      # just these

Needs a Python with qwen-tts (https://github.com/QwenLM/Qwen3-TTS) and a
CUDA GPU (about 5 GB free).
"""
import argparse
import json
import time
from pathlib import Path

import soundfile as sf
import torch
from qwen_tts import Qwen3TTSModel

HERE = Path(__file__).resolve().parent
DESIGN_MODEL = 'Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign'


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument('names', nargs='*', help='voices to (re)design (default: all)')
    ap.add_argument('--seed', type=int, default=7)
    args = ap.parse_args()
    cfg = json.loads((HERE / 'voices.json').read_text())
    names = args.names or list(cfg['voices'])
    unknown = [n for n in names if n not in cfg['voices']]
    if unknown:
        raise SystemExit(f'not in voices.json: {", ".join(unknown)}')

    model = Qwen3TTSModel.from_pretrained(DESIGN_MODEL, device_map='cuda:0', dtype=torch.bfloat16)
    for name in names:
        torch.manual_seed(args.seed)
        t0 = time.time()
        wavs, sr = model.generate_voice_design(text=cfg['ref_text'], language='English', instruct=cfg['voices'][name])
        out = HERE / 'voices' / f'{name}.flac'
        sf.write(out, wavs[0], sr)
        print(f'{out.relative_to(HERE.parent.parent)}: {len(wavs[0]) / sr:.1f} s of speech in {time.time() - t0:.1f} s', flush=True)


if __name__ == '__main__':
    main()
