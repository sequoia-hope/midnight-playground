"""Crops of the HUD's parts, JS above Rust, enlarged, for looking at the
details (WP 6.3, D820+).

  python3 tools/parity/hud-crops.py <js.png> <rust.png> <out.png> <x,y,w,h> [<x,y,w,h> ...] [--scale 2]

Boxes are in CSS px; each picture is scaled to CSS px first (the JS page
shot is at the device's pixel ratio, the Rust canvas at its own).
"""
import sys
from PIL import Image

args = sys.argv[1:]
scale = 2
if '--scale' in args:
    i = args.index('--scale')
    scale = float(args[i + 1])
    del args[i:i + 2]
js, rust, out, *boxes = args
css_w = None
pics = []
for p in (js, rust):
    pics.append(Image.open(p).convert('RGB'))
# The CSS width: the narrower of the two shots' widths, in the ratio of the
# JS shot (desktop 1×; phones 3×).
w_js, h_js = pics[0].size
w_rs, h_rs = pics[1].size
# Phone shots: the JS page at 3×, the Rust canvas at its own ratio (1×
# without High quality): both to the smaller.
css = (w_js, h_js) if w_js <= w_rs else (w_rs, h_rs)
pics = [im.resize((int(css[0]), int(css[1])), Image.LANCZOS) for im in pics]
tiles = []
for b in boxes:
    x, y, w, h = [int(v) for v in b.split(',')]
    pair = [im.crop((x, y, x + w, y + h)).resize((int(w * scale), int(h * scale)), Image.LANCZOS) for im in pics]
    col = Image.new('RGB', (pair[0].width, pair[0].height * 2 + 4), (255, 0, 255))
    col.paste(pair[0], (0, 0))
    col.paste(pair[1], (0, pair[0].height + 4))
    tiles.append(col)
W = sum(t.width for t in tiles) + 4 * (len(tiles) - 1)
H = max(t.height for t in tiles)
sheet = Image.new('RGB', (W, H), (40, 40, 40))
x = 0
for t in tiles:
    sheet.paste(t, (x, 0))
    x += t.width + 4
sheet.save(out)
print(out, sheet.size)
