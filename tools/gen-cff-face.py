#!/usr/bin/env python3
"""Generate fixtures/synthetic/resubset/face-cff.otf and face.cff: a synthetic
CFF face for the CFF re-subset tests (docs/LEGAL.md §5: every outline is
written here).

Glyphs: .notdef A B C E acute Eacute. A and B call local subr 0 (a bar);
C calls global subr 0 (a box); Eacute is `seac` (E + acute, Standard codes
69 and 194, Adobe TN5177 Appendix C). StandardEncoding is the built-in
encoding. No timestamps vary, so a rerun is byte-identical.

Run: python tools/gen-cff-face.py (needs fontTools)
"""

import io
from pathlib import Path

from fontTools.cffLib import GlobalSubrsIndex, SubrsIndex
from fontTools.fontBuilder import FontBuilder
from fontTools.misc.psCharStrings import T2CharString

OUT = Path(__file__).resolve().parent.parent / "fixtures/synthetic/resubset"
ORDER = [".notdef", "A", "B", "C", "E", "acute", "Eacute"]
BAR = [0, 0, "rmoveto", 600, 0, 0, 100, -600, 0, "rlineto", "endchar"]


def charstrings(private, gsubrs):
    cs = lambda p: T2CharString(program=p, private=private, globalSubrs=gsubrs)
    return {
        ".notdef": cs([500, "endchar"]),
        # Local subr 0, biased by -107 (fewer than 1240 subrs).
        "A": cs([600, 50, 0, "rmoveto", 250, 700, 250, -700, "rlineto", -107, "callsubr", "endchar"]),
        "B": cs([600, 50, 0, "rmoveto", 0, 700, 300, 0, 0, -700, "rlineto", -107, "callsubr", "endchar"]),
        "C": cs([600, -107, "callgsubr"]),
        "E": cs([600, 50, 0, "rmoveto", 0, 700, 400, 0, "rlineto", -107, "callsubr", "endchar"]),
        "acute": cs([300, 100, 750, "rmoveto", 100, 120, 40, -20, "rlineto", "endchar"]),
        "Eacute": cs([600, 0, 0, 69, 194, "endchar"]),
    }


def build() -> bytes:
    fb = FontBuilder(1000, isTTF=False)
    fb.setupGlyphOrder(ORDER)
    fb.setupCharacterMap({0x41: "A", 0x42: "B", 0x43: "C", 0x45: "E",
                          0xB4: "acute", 0xC9: "Eacute"})
    fb.setupCFF("SynthCff", {"FullName": "Synth Cff"}, {n: T2CharString() for n in ORDER}, {})
    cff = fb.font["CFF "].cff
    top = cff.topDictIndex[0]
    private = top.Private
    subrs = SubrsIndex()
    subrs.append(T2CharString(program=[0, -20, "rmoveto", 600, 0, 0, 20, -600, 0, "rlineto", "return"]))
    private.Subrs = subrs
    gsubrs = cff.GlobalSubrs
    gsubrs.append(T2CharString(program=BAR[:-1] + ["endchar"]))
    for name, c in charstrings(private, gsubrs).items():
        top.CharStrings[name] = c
    fb.setupHorizontalMetrics({n: (600, 0) for n in ORDER})
    fb.setupHorizontalHeader(ascent=800, descent=-200)
    fb.setupNameTable({"familyName": "Synth Cff", "styleName": "Regular"})
    fb.setupOS2(fsType=0)
    fb.setupPost()
    fb.font["head"].created = fb.font["head"].modified = 0
    buf = io.BytesIO()
    fb.font.save(buf)
    return buf.getvalue()


def main() -> None:
    from fontTools.ttLib import TTFont

    otf = build()
    font = TTFont(io.BytesIO(otf))
    bare = font.reader["CFF "]
    for name, data in [("face-cff.otf", otf), ("face.cff", bare)]:
        (OUT / name).write_bytes(data)
        print(f"wrote {OUT / name} ({len(data)} B)")


if __name__ == "__main__":
    main()
