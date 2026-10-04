"""Mean luminance of boxes of race pictures, and a side-by-side strip.

python3 tools/parity/lum.py --box x0,y0,x1,y1 [--box ...] --out strip.png a.png b.png ...
Boxes are fractions of the picture (0..1). Luminance: sRGB decoded, Rec. 709
weights, mean of the linear values, reported also as 8-bit sRGB.
"""
import argparse
import numpy as np
from PIL import Image, ImageDraw

ap = argparse.ArgumentParser()
ap.add_argument('--box', action='append', default=[])
ap.add_argument('--out')
ap.add_argument('files', nargs='+')
a = ap.parse_args()
boxes = [tuple(float(v) for v in b.split(',')) for b in a.box]


def lin(c):
    c = c / 255.0
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def srgb8(y):
    v = 12.92 * y if y <= 0.0031308 else 1.055 * y ** (1 / 2.4) - 0.055
    return 255 * v


ims = []
for f in a.files:
    im = Image.open(f).convert('RGB')
    arr = np.asarray(im).astype(np.float64)
    L = lin(arr) @ np.array([0.2126, 0.7152, 0.0722])
    h, w = L.shape
    vals = []
    for (x0, y0, x1, y1) in boxes:
        r = L[int(y0 * h):int(y1 * h), int(x0 * w):int(x1 * w)]
        vals.append(f'{r.mean():.4f} ({srgb8(r.mean()):.0f})')
    print(f'{f.split("/")[-1]:48s} whole {L.mean():.4f}  ' + '  '.join(vals))
    ims.append(im)
if a.out:
    H = 600
    rs = [im.resize((round(im.width * H / im.height), H)) for im in ims]
    strip = Image.new('RGB', (sum(r.width for r in rs) + 8 * (len(rs) - 1), H), (255, 255, 255))
    x = 0
    for r in rs:
        d = ImageDraw.Draw(r)
        for (x0, y0, x1, y1) in boxes:
            d.rectangle([x0 * r.width, y0 * H, x1 * r.width, y1 * H], outline=(255, 0, 255))
        strip.paste(r, (x, 0))
        x += r.width + 8
    strip.save(a.out)
