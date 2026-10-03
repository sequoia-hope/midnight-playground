#!/usr/bin/env python3
"""Rebuilds the bundled fonts of assets/fonts/ from google/fonts (DECISIONS
D370-D373).

    python3 assets/fonts/subset.py

Each file is fetched from the google/fonts repository at the pinned commit
below (cached in target/fonts-upstream/), checked against its SHA-256, and
cut down with fontTools' subsetter to the characters the game draws: Basic
Latin, Latin-1 and a few symbols (the JS signs use · — → ●). Outlines,
variation data, hinting instructions, every OpenType layout feature, the
legacy kern table, the names and the .notdef box are kept, so a kept
character draws exactly as in the full file; only the other characters go.
The licences are copied unchanged. Yellowtail (Apache 2.0, 62 KB) is copied
whole. Needs fontTools (pip install fonttools).
"""

import hashlib
import shutil
import sys
import urllib.parse
import urllib.request
from pathlib import Path

from fontTools import subset

COMMIT = "9710da1eacb3be272583c3224dcb70f9da6eadbb"  # google/fonts main, 2026-09-30
HERE = Path(__file__).resolve().parent
CACHE = HERE.parent.parent / "target" / "fonts-upstream" / COMMIT[:12]

# The characters kept: Basic Latin, Latin-1, the dashes, quotes, bullet and
# ellipsis, the four arrows, the minus sign and the black circle.
TEXT = (
    list(range(0x20, 0x7F))
    + list(range(0xA0, 0x100))
    + [0x2013, 0x2014, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2026]
    + [0x2190, 0x2191, 0x2192, 0x2193, 0x2212, 0x25CF]
)
# The fallback face (Arimo) carries only what some mapped face lacks: the
# symbols outside Latin-1.
SYMBOLS = [u for u in TEXT if u > 0xFF]

# (upstream path, unicodes or None to copy whole, SHA-256 of the upstream file)
FILES = [
    ("ofl/roboto/Roboto[wdth,wght].ttf", TEXT, "d7598e12c5dbef095ff8272cfc55da0250bd07fbdecbac8a530b9b277872a134"),
    ("ofl/roboto/OFL.txt", None, "061402327a96aadb0bfb694a960ed289ecd38d383e396243831ab81feb109c41"),
    ("ofl/robotocondensed/RobotoCondensed[wght].ttf", TEXT, "dace262afcee68a5276f200d8026c57221735c0118ab5fda8c2c0d3dc409a8d0"),
    ("ofl/robotocondensed/RobotoCondensed-Italic[wght].ttf", TEXT, "78f643b1923008b00dfc9b371a2ecd4d80a017722925f1a3fac9940be56d1b7d"),
    ("ofl/robotocondensed/OFL.txt", None, "0e4cc6ece88573545be2ed25835363662a6182ba4a4c1b5c8feda52add30e8a6"),
    ("ofl/arimo/Arimo[wght].ttf", SYMBOLS, None),
    ("ofl/arimo/OFL.txt", None, None),
    ("ofl/gelasio/Gelasio[wght].ttf", TEXT, None),
    ("ofl/gelasio/Gelasio-Italic[wght].ttf", TEXT, None),
    ("ofl/gelasio/OFL.txt", None, None),
    ("apache/yellowtail/Yellowtail-Regular.ttf", None, None),
    ("apache/yellowtail/LICENSE.txt", None, None),
    ("ofl/caveat/Caveat[wght].ttf", TEXT, None),
    ("ofl/caveat/OFL.txt", None, None),
    ("ofl/courierprime/CourierPrime-Regular.ttf", TEXT, None),
    ("ofl/courierprime/CourierPrime-Bold.ttf", TEXT, None),
    ("ofl/courierprime/CourierPrime-Italic.ttf", TEXT, None),
    ("ofl/courierprime/CourierPrime-BoldItalic.ttf", TEXT, None),
    ("ofl/courierprime/OFL.txt", None, None),
    ("ofl/rajdhani/Rajdhani-Regular.ttf", TEXT, None),
    ("ofl/rajdhani/Rajdhani-Medium.ttf", TEXT, None),
    ("ofl/rajdhani/Rajdhani-SemiBold.ttf", TEXT, None),
    ("ofl/rajdhani/Rajdhani-Bold.ttf", TEXT, None),
    ("ofl/rajdhani/OFL.txt", None, None),
]


def fetch(rel):
    dst = CACHE / rel
    if not dst.exists():
        dst.parent.mkdir(parents=True, exist_ok=True)
        url = f"https://raw.githubusercontent.com/google/fonts/{COMMIT}/{urllib.parse.quote(rel)}"
        with urllib.request.urlopen(url) as r:
            dst.write_bytes(r.read())
        print("  fetched", rel)
    return dst


def options():
    o = subset.Options()
    o.layout_features = ["*"]
    o.layout_scripts = ["*"]
    o.legacy_kern = True
    o.hinting = True
    o.notdef_outline = True
    o.name_IDs = ["*"]
    o.name_languages = ["*"]
    o.name_legacy = True
    o.glyph_names = True
    o.recalc_timestamp = False
    o.prune_unicode_ranges = False
    return o


def main():
    for rel, text, want in FILES:
        src = fetch(rel)
        data = src.read_bytes()
        if want and hashlib.sha256(data).hexdigest() != want:
            sys.exit(f"{rel}: SHA-256 differs from the pinned file")
        dst = HERE / rel.split("/", 1)[1]
        dst.parent.mkdir(parents=True, exist_ok=True)
        if text is None:
            shutil.copyfile(src, dst)
            continue
        o = options()
        font = subset.load_font(str(src), o)
        s = subset.Subsetter(o)
        s.populate(unicodes=text)
        s.subset(font)
        subset.save_font(font, str(dst), o)
        print(f"  {dst.relative_to(HERE)}: {len(data) // 1024} KB -> {dst.stat().st_size // 1024} KB")


if __name__ == "__main__":
    main()
