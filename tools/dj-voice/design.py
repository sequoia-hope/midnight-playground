"""Design the DJ voices with Qwen3-TTS VoiceDesign.

Every DJ in voices.json has a few candidate voices, each a plain-English
description. This speaks the DJ's ref_text in each candidate and saves
voices/<candidate>.flac: the reference clip render.py clones every line
from, so a DJ's whole script comes out in one consistent voice. Listen to
them in tools/dj-voice.html, set "chosen" in voices.json, then run
render.py.

    python tools/dj-voice/design.py                 # every candidate not designed yet
    python tools/dj-voice/design.py marisol-smoky   # just these (re-designs them)
    python tools/dj-voice/design.py --force         # every candidate again

Needs a Python with qwen-tts (https://github.com/QwenLM/Qwen3-TTS) and a
CUDA GPU (about 5 GB free), the same as tools/radio-voice/.
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
    ap.add_argument('names', nargs='*', help='candidates to (re)design (default: those not designed yet)')
    ap.add_argument('--force', action='store_true', help='re-design every candidate')
    ap.add_argument('--seed', type=int, default=7)
    args = ap.parse_args()
    cfg = json.loads((HERE / 'voices.json').read_text())
    cands = {c: (dj, desc) for dj, d in cfg['djs'].items() for c, desc in d['candidates'].items()}
    unknown = [n for n in args.names if n not in cands]
    if unknown:
        raise SystemExit(f'not in voices.json: {", ".join(unknown)}')
    out_dir = HERE / 'voices'
    out_dir.mkdir(exist_ok=True)
    names = args.names or [c for c in cands if args.force or not (out_dir / f'{c}.flac').exists()]
    if not names:
        print('every candidate voice is designed already (--force to redo them)')
        return

    model = Qwen3TTSModel.from_pretrained(DESIGN_MODEL, device_map='cuda:0', dtype=torch.bfloat16)
    for name in names:
        dj, desc = cands[name]
        torch.manual_seed(args.seed)
        t0 = time.time()
        wavs, sr = model.generate_voice_design(text=cfg['djs'][dj]['ref_text'], language='English', instruct=desc)
        out = out_dir / f'{name}.flac'
        sf.write(out, wavs[0], sr)
        print(f'{out.relative_to(HERE.parent.parent)}: {len(wavs[0]) / sr:.1f} s of speech in {time.time() - t0:.1f} s', flush=True)


if __name__ == '__main__':
    main()
