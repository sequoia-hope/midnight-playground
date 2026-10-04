#!/usr/bin/env python3
"""Makes the oblique Rajdhani Bold the menus draw their italic headings in
(DECISIONS D571).

    python3 assets/fonts/oblique.py

The JS menus ask for Rajdhani in italic (`.logo`, `.title`, `.lvl-name`,
`.pos` and the rest: `font-style: italic`), and Rajdhani has no italic, so
Chrome slants the upright face itself: Skia's fake italic, a horizontal
skew of -1/4 in device space (x' = x + y / 4 with y up, about the
baseline), with the advances unchanged. Bevy's text draws no synthetic
styles, so this script bakes the same skew into a copy of the bundled
Rajdhani-Bold.ttf (the menus' italics are all 700 or heavier, which is the
Bold face in Chrome too) and names it Rajdhani Oblique (Bold). Hinting
instructions are dropped (they would hint the upright outline). The output
is deterministic. Needs fontTools (pip install fonttools).
"""

from pathlib import Path

from fontTools.ttLib import TTFont

HERE = Path(__file__).resolve().parent
SRC = HERE / "rajdhani" / "Rajdhani-Bold.ttf"
OUT = HERE / "rajdhani" / "Rajdhani-BoldOblique.ttf"
SKEW = 0.25  # Skia's SK_Scalar1 / 4 (Blink's synthetic italic)


def main():
    font = TTFont(SRC, recalcTimestamp=False)
    glyf = font["glyf"]
    hmtx = font["hmtx"]
    order = font.getGlyphOrder()
    # Simple glyphs: skew each point. Composites: their parts are skewed
    # glyphs already; skew the offsets they are placed at.
    for name in order:
        g = glyf[name]
        if g.isComposite():
            for c in g.components:
                c.x = round(c.x + SKEW * c.y)
        elif g.numberOfContours > 0:
            coords = g.coordinates
            for i in range(len(coords)):
                x, y = coords[i]
                coords[i] = (round(x + SKEW * y), y)
        if hasattr(g, "program"):
            from fontTools.ttLib.tables import ttProgram

            g.program = ttProgram.Program()
            g.program.fromBytecode(b"")
    for name in order:
        g = glyf[name]
        if g.isComposite() or g.numberOfContours > 0:
            g.recalcBounds(glyf)
            adv, _ = hmtx[name]
            hmtx[name] = (adv, g.xMin)
    for tag in ("fpgm", "prep", "cvt ", "hdmx", "LTSH", "VDMX"):
        if tag in font:
            del font[tag]
    # Its own family, so the text engine picks it by name: Bevy's text asks
    # for "Rajdhani Oblique" where the CSS asks for italic Rajdhani.
    font["post"].italicAngle = -14.036243467926479  # atan(1/4)
    font["hhea"].caretSlopeRise = 4
    font["hhea"].caretSlopeRun = 1
    for rec in font["name"].names:
        if rec.nameID in (1, 16):
            rec.string = "Rajdhani Oblique"
        elif rec.nameID in (2, 17):
            rec.string = "Bold"
        elif rec.nameID == 4:
            rec.string = "Rajdhani Oblique Bold"
        elif rec.nameID == 6:
            rec.string = "RajdhaniOblique-Bold"
    font.save(OUT, reorderTables=True)
    print(f"{OUT.relative_to(HERE.parent.parent)}: {OUT.stat().st_size} bytes")


if __name__ == "__main__":
    main()
