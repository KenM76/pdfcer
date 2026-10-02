"""Generate fixtures/synthetic/text/augment/*.ttf for decision 173 (Pass 430.1).

A synthetic "installed face" and subsets cut from it, so program surgery can
be tested without real font bytes:

  face.ttf                 .notdef A B C D E acute Eacute(composite E+acute),
                           hinted (fpgm/prep/cvt + per-glyph instructions),
                           post 2.0, cmap (0,3) + (3,1)
  subset.ttf               .notdef A B C cut from face.ttf: same outlines,
                           advances, instructions and fpgm/prep/cvt
  subset-other-fpgm.ttf    as subset.ttf, but its fpgm differs from the face's
  face-b-differs.ttf       as face.ttf, but glyph B has another outline
  subset-empty-slot.ttf    .notdef A B C D, D mapped but with no outline
                           (how Word subsets an unshown character)

Deterministic: fixed head timestamps, no fontTools-version-dependent tables.
"""

from __future__ import annotations

import array
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "fixtures" / "synthetic" / "text" / "augment"

NAME = "pdfcerAugFace"
UPEM = 1000
TIMESTAMP = 3873733256

# name -> (unicode, advance, contours)
SIMPLE = {
    "A": (0x41, 600, [[(50, 0), (300, 700), (550, 0)]]),
    "B": (0x42, 620, [[(80, 0), (80, 700), (500, 700), (540, 350), (500, 0)]]),
    "C": (0x43, 640, [[(560, 100), (300, 0), (60, 350), (300, 700), (560, 600)]]),
    "D": (0x44, 660, [[(80, 0), (80, 700), (400, 700), (600, 350), (400, 0)]]),
    "E": (0x45, 560, [[(80, 0), (80, 700), (500, 700), (500, 600), (180, 600), (180, 0)]]),
    "acute": (0xB4, 300, [[(100, 750), (200, 900), (250, 880), (140, 740)]]),
}
B_OTHER = [[(80, 0), (80, 700), (520, 700), (520, 0)]]
COMPOSITE = {"Eacute": (0xC9, 560, [("E", 0, 0), ("acute", 150, 0)])}
GLYPH_PROGRAM = [0xB0, 0x01, 0x2F]  # PUSHB[0] 1; MDAP[1] -- harmless, non-empty
FPGM = [0xB0, 0x00, 0x2C, 0xB0, 0x07, 0x2D]  # FDEF 0 { PUSHB 7 } ENDF
FPGM_OTHER = [0xB0, 0x00, 0x2C, 0xB0, 0x09, 0x2D]
PREP = [0xB0, 0x00, 0x2F]
CVT = [0, 700, -10]


def _glyph(contours, program):
    from fontTools.pens.ttGlyphPen import TTGlyphPen
    from fontTools.ttLib.tables import ttProgram

    pen = TTGlyphPen(None)
    for c in contours:
        pen.moveTo(c[0])
        for p in c[1:]:
            pen.lineTo(p)
        pen.closePath()
    g = pen.glyph()
    g.program = ttProgram.Program()
    g.program.fromBytecode(bytes(program))
    return g


def _composite(parts, glyphs):
    from fontTools.ttLib.tables._g_l_y_f import Glyph, GlyphComponent

    g = Glyph()
    g.numberOfContours = -1
    g.components = []
    for name, dx, dy in parts:
        c = GlyphComponent()
        c.glyphName, c.x, c.y, c.flags = name, dx, dy, 0x0004  # ROUND_XY_TO_GRID
        g.components.append(c)
    return g


def build(names, *, b_outline=None, fpgm=FPGM, empty=()) -> bytes:
    from fontTools.fontBuilder import FontBuilder
    from fontTools.ttLib import newTable
    from fontTools.ttLib.tables import ttProgram

    order = [".notdef"] + names
    fb = FontBuilder(UPEM, isTTF=True)
    fb.setupGlyphOrder(order)
    glyphs = {".notdef": _glyph([[(50, 0), (50, 700), (450, 700), (450, 0)]], [])}
    metrics = {".notdef": (500, 50)}
    cmap = {}
    for n in names:
        if n in SIMPLE:
            uni, adv, contours = SIMPLE[n]
            if n == "B" and b_outline is not None:
                contours = b_outline
            glyphs[n] = _glyph([] if n in empty else contours, [] if n in empty else GLYPH_PROGRAM)
        else:
            uni, adv, parts = COMPOSITE[n]
            glyphs[n] = _composite(parts, glyphs)
        cmap[uni] = n
        metrics[n] = (adv, 0)
    fb.setupGlyf(glyphs)
    glyf = fb.font["glyf"]
    for n in order:
        g = glyf[n]
        g.recalcBounds(glyf)
        metrics[n] = (metrics[n][0], getattr(g, "xMin", 0))
    fb.setupHorizontalMetrics(metrics)
    fb.setupHorizontalHeader(ascent=900, descent=-100)
    fb.setupCharacterMap(cmap)
    fb.setupNameTable({"familyName": NAME, "styleName": "Regular", "psName": NAME})
    fb.setupOS2(fsType=0, usWinAscent=900, usWinDescent=100)
    fb.setupPost(keepGlyphNames=True)
    fb.setupMaxp()
    for tag, data in (("fpgm", fpgm), ("prep", PREP)):
        t = newTable(tag)
        t.program = ttProgram.Program()
        t.program.fromBytecode(bytes(data))
        fb.font[tag] = t
    cvt = newTable("cvt ")
    cvt.values = array.array("h", CVT)
    fb.font["cvt "] = cvt
    fb.font["head"].created = fb.font["head"].modified = TIMESTAMP
    fb.font["head"].fontRevision = 1.0
    import io

    buf = io.BytesIO()
    fb.font.save(buf, reorderTables=True)
    return buf.getvalue()


ALL = ["A", "B", "C", "D", "E", "acute", "Eacute"]
SUBSET = ["A", "B", "C"]
VARIANTS = {
    "face.ttf": (ALL, {}),
    "subset.ttf": (SUBSET, {}),
    "subset-other-fpgm.ttf": (SUBSET, {"fpgm": FPGM_OTHER}),
    "face-b-differs.ttf": (ALL, {"b_outline": B_OTHER}),
    "subset-empty-slot.ttf": (SUBSET + ["D"], {"empty": ("D",)}),
}


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    for name, (names, kw) in VARIANTS.items():
        data = build(names, **kw)
        (OUT / name).write_bytes(data)
        print(f"wrote {name} ({len(data)} bytes)")


if __name__ == "__main__":
    main()
