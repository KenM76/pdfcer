#!/usr/bin/env python3
"""Generate fixtures/synthetic/resubset/face-cff.otf, face.cff and
face-cid.cff: a synthetic CFF face for the CFF re-subset tests
(docs/LEGAL.md §5: every outline is written here).

Glyphs: .notdef A B C E acute Eacute. A and B call local subr 0 (a bar);
C calls global subr 0 (a box); Eacute is `seac` (E + acute, Standard codes
69 and 194, Adobe TN5177 Appendix C). StandardEncoding is the built-in
encoding. No timestamps vary, so a rerun is byte-identical.

face-cid.cff is the same outlines CID-keyed (ROS Adobe-Identity-0, one
FDArray entry): glyphs 0-6 carry CIDs 0 1 2 3 5 8 9, gaps on purpose, and
Eacute is a plain `endchar` because `seac` is not allowed in a CIDFont
(TN5176 Appendix C).

face-cid-badcharset.cff is face-cid.cff with its charset's format byte set
to 3, which TN5176 Table 17 does not define, so the charset cannot be read.

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


def cid_keyed(bare: bytes) -> bytes:
    from fontTools.cffLib import CFFFontSet, FDArrayIndex, FDSelect, FontDict

    fs = CFFFontSet()
    fs.decompile(io.BytesIO(bare), None)
    top = fs[fs.fontNames[0]]
    names = [".notdef"] + [f"cid{c:05d}" for c in (1, 2, 3, 5, 8, 9)]
    top.CharStrings["Eacute"].program = [600, "endchar"]
    top.CharStrings.charStrings = {n: i for i, n in enumerate(names)}
    top.charset = names
    top.ROS = ("Adobe", "Identity", 0)
    top.CIDCount = 10
    fd = FontDict()
    fd.Private = top.Private
    fd.FontMatrix = [0.001, 0, 0, 0.001, 0, 0]
    top.FDArray = FDArrayIndex()
    top.FDArray.append(fd)
    top.FDSelect = FDSelect()
    top.FDSelect.format = 3
    top.FDSelect.gidArray = [0] * len(names)
    del top.Private

    class NoBBoxes:
        recalcBBoxes = False

    out = io.BytesIO()
    fs.compile(out, NoBBoxes())
    return out.getvalue()


def bad_charset(cff: bytes) -> bytes:
    """`cff` with its custom charset's format byte set to 3, which no charset has."""

    def index_end(at: int) -> tuple[int, int]:
        count = int.from_bytes(cff[at : at + 2], "big")
        size = cff[at + 2]
        offset = lambda i: int.from_bytes(cff[at + 3 + i * size : at + 3 + (i + 1) * size], "big")
        base = at + 3 + (count + 1) * size - 1
        return base + offset(0), base + offset(count)

    _, names_end = index_end(cff[2])
    at, end = index_end(names_end)
    operand, charset = 0, None
    while at < end:
        b = cff[at]
        if b == 12:
            at += 2
        elif b <= 21:
            if b == 15:
                charset = operand
            at += 1
        elif b == 28:
            operand = int.from_bytes(cff[at + 1 : at + 3], "big", signed=True)
            at += 3
        elif b == 29:
            operand = int.from_bytes(cff[at + 1 : at + 5], "big", signed=True)
            at += 5
        elif b == 30:
            at += 1
            while cff[at] & 0x0F != 0x0F and cff[at] >> 4 != 0x0F:
                at += 1
            at += 1
        elif b <= 246:
            operand = b - 139
            at += 1
        else:
            first = b - 247 if b <= 250 else -(b - 251)
            operand = first * 256 + (cff[at + 1] if b <= 250 else -cff[at + 1])
            operand += 108 if b <= 250 else -108
            at += 2
    assert charset is not None and charset > 2, charset
    out = bytearray(cff)
    out[charset] = 3
    return bytes(out)


def main() -> None:
    from fontTools.ttLib import TTFont

    otf = build()
    font = TTFont(io.BytesIO(otf))
    bare = font.reader["CFF "]
    cid = cid_keyed(bare)
    files = [
        ("face-cff.otf", otf),
        ("face.cff", bare),
        ("face-cid.cff", cid),
        ("face-cid-badcharset.cff", bad_charset(cid)),
    ]
    for name, data in files:
        (OUT / name).write_bytes(data)
        print(f"wrote {OUT / name} ({len(data)} B)")


if __name__ == "__main__":
    main()
