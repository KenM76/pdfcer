"""Generate fixtures/synthetic/text/word-shaped-subset.pdf and its
`-tounicode` twin (Pass 430.0).

The shape Microsoft Word gives an embedded TrueType subset: `/TrueType`,
`/WinAnsiEncoding`, nonsymbolic (`/Flags 32`), no `/ToUnicode`, and a program
whose `(3,1)` cmap and outlines cover MORE than the pages show, while
`/Widths` spans only the shown codes. Decision 172 route A makes those extra
characters typeable.

Program coverage (all reachable through the `(3,1)` cmap):
  A B C      shown on page 1 (page 2 shows A), /Widths 65..67
  D, endash  outlined but never shown            -> typeable by route A
  E          mapped to an EMPTY slot (a dropped glyph) -> refused
  space      empty outline, positive advance     -> typeable (blank glyph)
  Delta, Zhe outlined, no WinAnsi code           -> typeable by an allocated code

The `-differences` twin names its encoding as a dictionary whose
`/Differences` already claims code 127 (for a glyph the program lacks), so
allocation must skip it.

The `-tounicode` twin adds a `/ToUnicode` CMap covering only the shown codes
0x41..0x43; `-shared-tounicode` also draws page 2 through a second font dict
sharing that CMap, so it cannot be rewritten.

Advances differ per glyph so a test can tell an hmtx-derived width from a
guessed one. Synthetic, generated with fontTools; no real font bytes.
"""

from __future__ import annotations

import importlib.util
import io
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("gen_subset", HERE / "gen-subset-font-fixtures.py")
gen = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(gen)

BASE_FONT = "WORDSB+pdfceWordShape"
UPEM = 2048
# glyph name -> (unicode, advance in font units, outlined)
GLYPHS = {
    "space": (0x20, 569, False),
    "A": (0x41, 1366, True),
    "B": (0x42, 1229, True),
    "C": (0x43, 1479, True),
    "D": (0x44, 1343, True),
    "E": (0x45, 1100, False),
    "endash": (0x2013, 1024, True),
    "Delta": (0x0394, 1253, True),
    "Zhe": (0x0416, 1700, True),
}


HEAD_TIMESTAMP = 3873733256


def build_program() -> bytes:
    from fontTools.fontBuilder import FontBuilder
    from fontTools.pens.ttGlyphPen import TTGlyphPen

    order = [".notdef"] + list(GLYPHS)
    fb = FontBuilder(UPEM, isTTF=True)
    fb.setupGlyphOrder(order)
    glyphs = {".notdef": TTGlyphPen(None).glyph()}
    for i, (name, (_, adv, outlined)) in enumerate(GLYPHS.items()):
        pen = TTGlyphPen(None)
        if outlined:
            top = 600 + 100 * i
            pen.moveTo((100, 0))
            pen.lineTo((adv - 100, 0))
            pen.lineTo((adv - 100, top))
            pen.lineTo((100, top))
            pen.closePath()
        glyphs[name] = pen.glyph()
    fb.setupGlyf(glyphs)
    metrics = {".notdef": (1024, 0)}
    metrics.update({n: (adv, 100) for n, (_, adv, _) in GLYPHS.items()})
    fb.setupHorizontalMetrics(metrics)
    fb.setupHorizontalHeader(ascent=1854, descent=-434)
    fb.setupNameTable({"familyName": "pdfceWordShape", "styleName": "Regular"})
    fb.setupCharacterMap({u: n for n, (u, _, _) in GLYPHS.items()})
    fb.setupOS2(sTypoAscender=1491, sTypoDescender=-431)
    fb.setupPost(keepGlyphNames=False)
    # Pinned so regeneration is byte-identical (fontTools stamps "now").
    fb.font["head"].created = fb.font["head"].modified = HEAD_TIMESTAMP
    buf = io.BytesIO()
    fb.font.save(buf)
    return buf.getvalue()


def widths(codes: str) -> str:
    return " ".join(str(round(GLYPHS[c][1] * 1000 / UPEM)) for c in codes)


TO_UNICODE = (
    b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n"
    b"/CMapName /Adobe-Identity-UCS def /CMapType 2 def\n"
    b"1 begincodespacerange <00> <FF> endcodespacerange\n"
    b"1 beginbfrange <41> <43> <0041> endbfrange\n"
    b"endcmap CMapName currentdict /CMap defineresource pop end end\n"
)


DIFFERENCES = "<< /Type /Encoding /BaseEncoding /WinAnsiEncoding /Differences [127 /uni2126] >>"


def build_pdf(to_unicode: bool = False, shared: bool = False, differences: bool = False) -> bytes:
    ttf = build_program()
    page1 = b"BT\n/F0 24 Tf\n72 600 Td\n(ABC) Tj\nET\n"
    page2 = b"BT\n/F0 24 Tf\n72 600 Td\n(A) Tj\nET\n"
    objects = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: (
            f"<< /Type /Pages /Kids [3 0 R 8 0 R] /Count 2 "
            f"/MediaBox [0 0 {gen.PAGE_WIDTH} {gen.PAGE_HEIGHT}] "
            f"/Resources << /Font << /F0 5 0 R >> >> >>"
        ).encode("ascii"),
        3: b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>",
        4: gen.raw_stream(page1, ""),
        5: (
            f"<< /Type /Font /Subtype /TrueType /BaseFont /{BASE_FONT} "
            f"/FirstChar 65 /LastChar 67 /Widths [{widths('ABC')}] "
            f"/Encoding {DIFFERENCES if differences else '/WinAnsiEncoding'} /FontDescriptor 6 0 R"
            f"{' /ToUnicode 10 0 R' if to_unicode else ''} >>"
        ).encode("ascii"),
        6: (
            f"<< /Type /FontDescriptor /FontName /{BASE_FONT} "
            f"/Flags 32 /FontBBox [-1361 -665 4096 2060] /ItalicAngle 0 "
            f"/Ascent 905 /Descent -212 /CapHeight 716 /StemV 80 "
            f"/FontFile2 7 0 R >>"
        ).encode("ascii"),
        7: gen.raw_stream(ttf, f" /Length1 {len(ttf)}"),
        8: b"<< /Type /Page /Parent 2 0 R /Contents 9 0 R >>",
        9: gen.raw_stream(page2, ""),
    }
    if to_unicode:
        objects[10] = gen.raw_stream(TO_UNICODE, "")
    if shared:
        # Page 2 draws through a second font dict sharing the same /ToUnicode.
        objects[8] = (
            b"<< /Type /Page /Parent 2 0 R /Contents 9 0 R "
            b"/Resources << /Font << /F0 11 0 R >> >> >>"
        )
        objects[11] = objects[5].replace(f"/BaseFont /{BASE_FONT}".encode(), b"/BaseFont /Twin")
    return gen.serialize(objects)


VARIANTS = (
    ("word-shaped-subset.pdf", False, False, False),
    ("word-shaped-subset-tounicode.pdf", True, False, False),
    ("word-shaped-subset-shared-tounicode.pdf", True, True, False),
    ("word-shaped-subset-differences.pdf", False, False, True),
)


def main() -> int:
    out_dir = Path(sys.argv[1]) if len(sys.argv) > 1 else gen.OUT
    for name, tu, shared, differences in VARIANTS:
        path = out_dir / name
        path.write_bytes(build_pdf(tu, shared, differences))
        print(f"wrote {path} ({path.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
