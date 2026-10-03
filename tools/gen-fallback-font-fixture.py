"""Generate fixtures/synthetic/text/fallback-font.pdf and fallback-donor.ttf
(Pass 431.0, EditOptions::fallback).

The page:
  /F0  TrueType WinAnsi subset FBKAAA+pdfcerFbRun; shows "Qu5" (codes 81 117 53)
       and carries nothing else, so a space, the euro sign (WinAnsi 128) and
       U+2265 (no WinAnsi code at all) are refused by every route that keeps it.
  /F1  Helvetica, non-embedded WinAnsi; shows "Hi" on another line, so a
       fallback named "Helvetica" reuses it rather than adding a resource.

fallback-font-form.pdf shows the same two lines from a form XObject /Fm0
whose /Resources hold the fonts; fallback-font-inherited.pdf has the page's
/Resources inherited from the page-tree node.

fallback-donor.ttf is a face a fallback can embed a subset of: space, Q, u, 5,
the euro sign and U+2265, each a rectangle of its own height so outlines are
distinguishable. Synthetic, generated with fontTools; no real font bytes.

fallback-run-face-restricted.ttf (Pass 436.2, decision 178) is the donor's
glyphs under the run font's own name, pdfcerFbRun, with OS/2 fsType 2
(Restricted License embedding): the replacement-face ladder's exact-name
rung finds it by its name ID 6 and must skip it. fallback-donor.ttf has no
name ID 6, so the ladder names it from its family.

fallback-donor-cff.otf (Pass 436.5) carries the donor's glyphs as CFF
outlines in an OTTO wrapper, named pdfcerFbCff, so the CFF embedding route
(/CIDFontType0 + /FontFile3 /CIDFontType0C) has a donor.
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

UPEM = 1000
HEAD_TIMESTAMP = 3873733256
RUN_FONT = "FBKAAA+pdfcerFbRun"
# glyph name -> (unicode, advance, outlined)
RUN_GLYPHS = {
    "Q": (0x51, 720, True),
    "u": (0x75, 560, True),
    "five": (0x35, 540, True),
}
DONOR_GLYPHS = {
    "space": (0x20, 300, False),
    "Q": (0x51, 700, True),
    "u": (0x75, 550, True),
    "five": (0x35, 530, True),
    "Euro": (0x20AC, 610, True),
    "greaterequal": (0x2265, 640, True),
}


def build_program(
    glyph_table: dict, family: str, fs_type: int = 0, ps_name: str | None = None
) -> bytes:
    from fontTools.fontBuilder import FontBuilder
    from fontTools.pens.ttGlyphPen import TTGlyphPen

    fb = FontBuilder(UPEM, isTTF=True)
    fb.setupGlyphOrder([".notdef"] + list(glyph_table))
    glyphs = {".notdef": TTGlyphPen(None).glyph()}
    for i, (name, (_, adv, outlined)) in enumerate(glyph_table.items()):
        pen = TTGlyphPen(None)
        if outlined:
            top = 400 + 50 * i
            pen.moveTo((50, 0))
            pen.lineTo((adv - 50, 0))
            pen.lineTo((adv - 50, top))
            pen.lineTo((50, top))
            pen.closePath()
        glyphs[name] = pen.glyph()
    fb.setupGlyf(glyphs)
    metrics = {".notdef": (500, 0)}
    metrics.update({n: (adv, 50) for n, (_, adv, _) in glyph_table.items()})
    fb.setupHorizontalMetrics(metrics)
    fb.setupHorizontalHeader(ascent=900, descent=-200)
    names = {"familyName": family, "styleName": "Regular"}
    if ps_name:
        names["psName"] = ps_name
    fb.setupNameTable(names)
    fb.setupCharacterMap({u: n for n, (u, _, _) in glyph_table.items()})
    fb.setupOS2(sTypoAscender=800, sTypoDescender=-200, fsType=fs_type)
    fb.setupPost(keepGlyphNames=False)
    # Pinned so regeneration is byte-identical (fontTools stamps "now").
    fb.font["head"].created = fb.font["head"].modified = HEAD_TIMESTAMP
    buf = io.BytesIO()
    fb.font.save(buf)
    return buf.getvalue()


def build_cff_program(glyph_table: dict, family: str) -> bytes:
    """`build_program`'s glyphs as CFF outlines (OTTO)."""
    from fontTools.fontBuilder import FontBuilder
    from fontTools.pens.t2CharStringPen import T2CharStringPen

    fb = FontBuilder(UPEM, isTTF=False)
    fb.setupGlyphOrder([".notdef"] + list(glyph_table))
    charstrings = {".notdef": T2CharStringPen(500, None).getCharString()}
    for i, (name, (_, adv, outlined)) in enumerate(glyph_table.items()):
        pen = T2CharStringPen(adv, None)
        if outlined:
            top = 400 + 50 * i
            pen.moveTo((50, 0))
            pen.lineTo((adv - 50, 0))
            pen.lineTo((adv - 50, top))
            pen.lineTo((50, top))
            pen.closePath()
        charstrings[name] = pen.getCharString()
    fb.setupCFF(family, {"FullName": family}, charstrings, {})
    metrics = {".notdef": (500, 0)}
    metrics.update({n: (adv, 50) for n, (_, adv, _) in glyph_table.items()})
    fb.setupHorizontalMetrics(metrics)
    fb.setupHorizontalHeader(ascent=900, descent=-200)
    fb.setupNameTable({"familyName": family, "styleName": "Regular", "psName": family})
    fb.setupCharacterMap({u: n for n, (u, _, _) in glyph_table.items()})
    fb.setupOS2(sTypoAscender=800, sTypoDescender=-200, fsType=0)
    fb.setupPost(keepGlyphNames=False)
    fb.font["head"].created = fb.font["head"].modified = HEAD_TIMESTAMP
    buf = io.BytesIO()
    fb.font.save(buf)
    return buf.getvalue()


def build_pdf(layout: str = "page") -> bytes:
    """`layout` is "page", "form" or "inherited" (see the module doc)."""
    ttf = build_program(RUN_GLYPHS, "pdfcerFbRun")
    content = (
        b"BT\n/F0 24 Tf\n72 600 Td\n(Qu5) Tj\nET\n"
        b"BT\n/F1 12 Tf\n72 500 Td\n(Hi) Tj\nET\n"
    )
    widths = " ".join(
        str(RUN_GLYPHS[n][1]) if n else "0"
        for n in [
            {0x35: "five", 0x51: "Q", 0x75: "u"}.get(c) for c in range(0x35, 0x76)
        ]
    )
    fonts = b"<< /Font << /F0 5 0 R /F1 8 0 R >> >>"
    box = f"[0 0 {gen.PAGE_WIDTH} {gen.PAGE_HEIGHT}]"
    node_res = fonts if layout == "inherited" else b""
    page_res = {
        "page": fonts,
        "form": b"<< /XObject << /Fm0 9 0 R >> >>",
        "inherited": b"",
    }[layout]
    objects = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: (
            f"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox {box}"
        ).encode("ascii")
        + (b" /Resources " + node_res if node_res else b"")
        + b" >>",
        3: b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R"
        + (b" /Resources " + page_res if page_res else b"")
        + b" >>",
        4: gen.raw_stream(b"q /Fm0 Do Q\n" if layout == "form" else content, ""),
        5: (
            f"<< /Type /Font /Subtype /TrueType /BaseFont /{RUN_FONT} "
            f"/FirstChar 53 /LastChar 117 /Widths [{widths}] "
            f"/Encoding /WinAnsiEncoding /FontDescriptor 6 0 R >>"
        ).encode("ascii"),
        6: (
            f"<< /Type /FontDescriptor /FontName /{RUN_FONT} "
            f"/Flags 32 /FontBBox [0 -200 1000 900] /ItalicAngle 0 "
            f"/Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 "
            f"/FontFile2 7 0 R >>"
        ).encode("ascii"),
        7: gen.raw_stream(ttf, f" /Length1 {len(ttf)}"),
        8: b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
    }
    if layout == "form":
        objects[9] = gen.raw_stream(
            content,
            f" /Type /XObject /Subtype /Form /BBox {box} /Resources "
            + fonts.decode("ascii"),
        )
    return gen.serialize(objects)


def main() -> int:
    out_dir = Path(sys.argv[1]) if len(sys.argv) > 1 else gen.OUT
    written = []
    for layout, name in [
        ("page", "fallback-font.pdf"),
        ("form", "fallback-font-form.pdf"),
        ("inherited", "fallback-font-inherited.pdf"),
    ]:
        pdf = out_dir / name
        pdf.write_bytes(build_pdf(layout))
        written.append(pdf)
    donor = out_dir / "fallback-donor.ttf"
    donor.write_bytes(build_program(DONOR_GLYPHS, "pdfcerFbDonor"))
    restricted = out_dir / "fallback-run-face-restricted.ttf"
    restricted.write_bytes(build_program(DONOR_GLYPHS, "pdfcerFbRun", fs_type=2, ps_name="pdfcerFbRun"))
    cff = out_dir / "fallback-donor-cff.otf"
    cff.write_bytes(build_cff_program(DONOR_GLYPHS, "pdfcerFbCff"))
    for path in [*written, donor, restricted, cff]:
        print(f"wrote {path} ({path.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
