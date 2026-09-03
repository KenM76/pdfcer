#!/usr/bin/env python3
"""Generate the synthetic no-cmap CIDFontType2 RENDER fixture.

WHY THIS EXISTS
---------------
Regression cover for the embedded-TrueType render class: a `/Type0`
font, `/Encoding /Identity-H`, with a `/CIDFontType2` descendant that
carries an embedded subset TrueType (`FontFile2`) and a `/CIDToGIDMap`
stream — and, crucially, whose TrueType program has **no `cmap` table**.

A `cmap` is legitimately absent from a CIDFontType2 subset (ISO 32000-1
§9.7.4.2: glyph selection is CID -> GID via `/CIDToGIDMap`, not through
the font's own character map), so real CAD/Office producers
(SolidWorks / AutoCAD / Office) ship exactly this shape. The embedded
programs use the ordinary TrueType sfnt version `0x00010000`, whose
FIRST byte is `0x00` (NUL). NUL is PDF whitespace (Table 1); a
font-program format detector that trims leading whitespace before
sniffing the magic strips that NUL, shifts the data one byte to
`0x01 0x00 ...`, and misroutes the whole font into the bare-CFF parser
— which fails "offset out of bounds", skipping ALL of the drawing's
text while graphics render fine. This fixture reproduces the class so
that regression stays caught.

PROVENANCE / LICENSE
--------------------
`docs/LEGAL.md` §5 permits only synthetic or rights-cleared PDFs in
`fixtures/`. Every byte here is constructed from nothing:

- The embedded TrueType is BUILT here with `fontTools.fontBuilder` from
  glyph outlines defined in this file (a `.notdef` and one filled box).
  It contains NO third-party font data — in particular none of the
  Arial / Verdana / Century Gothic subsets from any real document. It
  is an original, trivial, CC0 / public-domain synthetic font.
- The PDF wrapper is written by the same minimal, library-free writer
  the other `tools/gen-*-fixtures.py` generators use (classic xref,
  exactly-20-byte entries, no `/ID`, no timestamps), so running this
  twice produces a byte-identical file and a fixture cannot inherit a
  bug from the code it exists to test.

Requires `fonttools` (dev-only; not a pdfcer runtime dependency).

Usage:  python tools/gen-cidfont-nocmap-fixtures.py
Output: fixtures/synthetic/text/cidfonttype2-nocmap-embedded.pdf
"""

from __future__ import annotations

import io
import sys
from pathlib import Path

PAGE_WIDTH = 612
PAGE_HEIGHT = 792

OUT = Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "text"


def build_nocmap_truetype() -> bytes:
    """Build a minimal TrueType with sfnt version 0x00010000 and NO cmap.

    Two glyphs: GID 0 `.notdef` (empty) and GID 1 `box` (a filled
    rectangle). The `cmap` table is deliberately removed after build to
    mirror the real subset-CIDFontType2 class (§9.7.4.2 makes it
    irrelevant — selection is CID->GID via `/CIDToGIDMap`).
    """
    from fontTools.fontBuilder import FontBuilder
    from fontTools.pens.ttGlyphPen import TTGlyphPen

    upem = 1000
    glyph_order = [".notdef", "box"]

    fb = FontBuilder(upem, isTTF=True)
    fb.setupGlyphOrder(glyph_order)
    # NOTE: intentionally no setupCharacterMap — the built font would get
    # a cmap only if we asked for one; we do not, and we assert its
    # absence below.

    pen = TTGlyphPen(None)
    # A filled box, on-curve corners, closed contour. y-up font units.
    pen.moveTo((100, 0))
    pen.lineTo((600, 0))
    pen.lineTo((600, 700))
    pen.lineTo((100, 700))
    pen.closePath()
    box = pen.glyph()

    notdef_pen = TTGlyphPen(None)
    notdef = notdef_pen.glyph()  # empty outline

    fb.setupGlyf({".notdef": notdef, "box": box})
    metrics = {".notdef": (upem, 0), "box": (upem, 100)}
    fb.setupHorizontalMetrics(metrics)
    fb.setupHorizontalHeader(ascent=800, descent=-200)
    fb.setupNameTable(
        {"familyName": "pdfceSyntheticBox", "styleName": "Regular"}
    )
    # fontBuilder requires a cmap present before OS/2 (it derives Unicode
    # ranges from it). We give it a throwaway one and DELETE it below, so
    # the shipped program is genuinely cmap-less — the real class.
    fb.setupCharacterMap({0x41: "box"})
    fb.setupOS2(sTypoAscender=800, sTypoDescender=-200)
    fb.setupPost()

    font = fb.font
    # Remove cmap if fontBuilder added a stub, to faithfully reproduce
    # the no-cmap class.
    if "cmap" in font:
        del font["cmap"]

    buf = io.BytesIO()
    font.save(buf)
    data = buf.getvalue()

    # Verify-don't-assume (R22): the fixture MUST carry the exact framing
    # this regression is about — sfnt version 0x00010000 and no cmap — or
    # it silently stops testing the thing it names.
    assert data[:4] == b"\x00\x01\x00\x00", f"sfnt magic was {data[:4]!r}"
    assert b"cmap" not in data[: 12 + 16 * _num_tables(data)], "cmap present"
    return data


def _num_tables(sfnt: bytes) -> int:
    return int.from_bytes(sfnt[4:6], "big")


def serialize(objects: dict[int, bytes]) -> bytes:
    """Classic xref layout, exactly-20-byte entries (§7.5.4). Identical
    discipline to the sibling generators."""
    out = bytearray(b"%PDF-1.7\n")
    out += b"%\xe2\xe3\xcf\xd3\n"
    highest = max(objects)
    offsets: dict[int, int] = {}
    for num in range(1, highest + 1):
        body = objects.get(num)
        if body is None:
            continue
        offsets[num] = len(out)
        out += f"{num} 0 obj\n".encode("ascii")
        out += body
        out += b"\nendobj\n"
    xref_at = len(out)
    out += f"xref\n0 {highest + 1}\n".encode("ascii")
    out += b"0000000000 65535 f \n"
    for num in range(1, highest + 1):
        if num in offsets:
            out += f"{offsets[num]:010d} 00000 n \n".encode("ascii")
        else:
            out += b"0000000000 65535 f \n"
    out += (
        f"trailer\n<< /Size {highest + 1} /Root 1 0 R >>\n"
        f"startxref\n{xref_at}\n%%EOF\n"
    ).encode("ascii")
    return bytes(out)


def raw_stream(body: bytes, extra: str) -> bytes:
    """Uncompressed stream with a correct `/Length` (and caller-supplied
    extra keys such as `/Length1`). Uncompressed so a failure cannot be
    blamed on the filter."""
    return (
        f"<< /Length {len(body)}{extra} >>\nstream\n".encode("ascii")
        + body
        + b"\nendstream"
    )


def cidfont_nocmap_embedded() -> bytes:
    """Type0 / Identity-H / CIDFontType2 / embedded no-cmap TrueType.

    The content stream selects CID 1 (Identity-H 2-byte code `0x0001`),
    which the `/CIDToGIDMap` maps to GID 1 (the box). If the box paints,
    the glyf-by-GID render path worked end to end.
    """
    ttf = build_nocmap_truetype()

    # CIDToGIDMap: 2 bytes per CID. CID 0 -> GID 0, CID 1 -> GID 1.
    cid_to_gid = bytes([0x00, 0x00, 0x00, 0x01])

    # Draw the box big, near the top of the page.
    content = (
        b"BT\n"
        b"/F0 200 Tf\n"
        b"100 400 Td\n"
        b"<0001> Tj\n"
        b"ET\n"
    )

    objects: dict[int, bytes] = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: (
            f"<< /Type /Pages /Kids [3 0 R] /Count 1 "
            f"/MediaBox [0 0 {PAGE_WIDTH} {PAGE_HEIGHT}] "
            f"/Resources << /Font << /F0 5 0 R >> >> >>"
        ).encode("ascii"),
        3: b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>",
        4: raw_stream(content, ""),
        # Type0 wrapper.
        5: (
            b"<< /Type /Font /Subtype /Type0 /BaseFont /ABCDEF+pdfceSyntheticBox "
            b"/Encoding /Identity-H /DescendantFonts [6 0 R] >>"
        ),
        # CIDFontType2 descendant.
        6: (
            b"<< /Type /Font /Subtype /CIDFontType2 "
            b"/BaseFont /ABCDEF+pdfceSyntheticBox "
            b"/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> "
            b"/FontDescriptor 7 0 R /CIDToGIDMap 8 0 R /DW 1000 >>"
        ),
        # FontDescriptor pointing at the embedded program.
        7: (
            b"<< /Type /FontDescriptor /FontName /ABCDEF+pdfceSyntheticBox "
            b"/Flags 4 /FontBBox [0 -200 1000 800] /ItalicAngle 0 "
            b"/Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 "
            b"/FontFile2 9 0 R >>"
        ),
        # CIDToGIDMap stream.
        8: raw_stream(cid_to_gid, ""),
        # The embedded TrueType, uncompressed. /Length1 == decoded length.
        9: raw_stream(ttf, f" /Length1 {len(ttf)}"),
    }
    return serialize(objects)


def cidfont_with_tounicode() -> bytes:
    """The same composite font, but carrying an INJECTIVE `/ToUnicode`.

    WHY THIS EXISTS
    ---------------
    The sibling fixture above has no `/ToUnicode`, so its text is
    undecodable — and that makes `edit-text --find` fail with "not found"
    BEFORE the composite check is ever reached. The R-INV-4 refusal message
    was therefore unreachable through the CLI: a refusal nobody could
    trigger, which is the same shape as a guard behind an unpassable filter
    (R96).

    With a `/ToUnicode` the text is findable, the edit locates it, and the
    composite refusal fires where it is supposed to — so the message can
    actually be read by whoever has to act on it.

    The CMap is deliberately injective (one CID, one scalar), because that is
    the arm standing rule R110 makes interesting: this font's map CAN be
    inverted, so the refusal must say pdfcer is the limitation rather than the
    font. A non-injective variant would exercise a different arm and is left
    for when that message needs proving.
    """
    ttf = build_nocmap_truetype()
    cid_to_gid = bytes([0x00, 0x00, 0x00, 0x01])

    # CID 1 -> U+0041 'A'. One entry, trivially injective.
    tounicode = (
        b"/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n"
        b"/CMapName /pdfcer-Identity-UCS def\n/CMapType 2 def\n"
        b"1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n"
        b"1 beginbfchar\n<0001> <0041>\nendbfchar\n"
        b"endcmap\nend\nend\n"
    )

    content = b"BT\n/F0 48 Tf\n72 600 Td\n<0001> Tj\nET\n"

    objects: dict[int, bytes] = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: (
            f"<< /Type /Pages /Kids [3 0 R] /Count 1 "
            f"/MediaBox [0 0 {PAGE_WIDTH} {PAGE_HEIGHT}] "
            f"/Resources << /Font << /F0 5 0 R >> >> >>"
        ).encode("ascii"),
        3: b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>",
        4: raw_stream(content, ""),
        5: (
            b"<< /Type /Font /Subtype /Type0 /BaseFont /ABCDEF+pdfceSyntheticBox "
            b"/Encoding /Identity-H /DescendantFonts [6 0 R] /ToUnicode 10 0 R >>"
        ),
        6: (
            b"<< /Type /Font /Subtype /CIDFontType2 "
            b"/BaseFont /ABCDEF+pdfceSyntheticBox "
            b"/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> "
            b"/FontDescriptor 7 0 R /CIDToGIDMap 8 0 R /DW 1000 >>"
        ),
        7: (
            b"<< /Type /FontDescriptor /FontName /ABCDEF+pdfceSyntheticBox "
            b"/Flags 4 /FontBBox [0 -200 1000 800] /ItalicAngle 0 "
            b"/Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 "
            b"/FontFile2 9 0 R >>"
        ),
        8: raw_stream(cid_to_gid, ""),
        9: raw_stream(ttf, f" /Length1 {len(ttf)}"),
        10: raw_stream(tounicode, ""),
    }
    return serialize(objects)



def cidfont_noninjective_tounicode() -> bytes:
    """The same composite font carrying a NON-INJECTIVE `/ToUnicode`.

    Two CIDs map to the SAME character (U+0041), so the inverse is not a
    function: asked to write 'A' back, pdfcer cannot know whether the file
    means CID 1 or CID 2, and either choice is a guess that renders as a
    real, wrong glyph.

    This fixture exists because composite editing WORKS as of Pass 29.0.
    Before that any composite font exercised the R-INV-4 refusal; now the
    refusal fires only for fonts whose map genuinely cannot be inverted, so
    testing it needs one of those. Its injective sibling is now the EDITABLE
    case, and the no-`/ToUnicode` one cannot serve: its text does not decode
    at all, so no anchor is ever found and `NoMatch` is the honest answer —
    the test would pass for the wrong reason.
    """
    ttf = build_nocmap_truetype()
    cid_to_gid = bytes([0x00, 0x00, 0x00, 0x01])

    # CID 1 -> U+0041 'A'. One entry, trivially injective.
    tounicode = (
        b"/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n"
        b"/CMapName /pdfcer-Identity-UCS def\n/CMapType 2 def\n"
        b"1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n"
        b"2 beginbfchar\n<0001> <0041>\n<0002> <0041>\nendbfchar\n"
        b"endcmap\nend\nend\n"
    )

    content = b"BT\n/F0 48 Tf\n72 600 Td\n<0001> Tj\nET\n"

    objects: dict[int, bytes] = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: (
            f"<< /Type /Pages /Kids [3 0 R] /Count 1 "
            f"/MediaBox [0 0 {PAGE_WIDTH} {PAGE_HEIGHT}] "
            f"/Resources << /Font << /F0 5 0 R >> >> >>"
        ).encode("ascii"),
        3: b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>",
        4: raw_stream(content, ""),
        5: (
            b"<< /Type /Font /Subtype /Type0 /BaseFont /ABCDEF+pdfceSyntheticBox "
            b"/Encoding /Identity-H /DescendantFonts [6 0 R] /ToUnicode 10 0 R >>"
        ),
        6: (
            b"<< /Type /Font /Subtype /CIDFontType2 "
            b"/BaseFont /ABCDEF+pdfceSyntheticBox "
            b"/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> "
            b"/FontDescriptor 7 0 R /CIDToGIDMap 8 0 R /DW 1000 >>"
        ),
        7: (
            b"<< /Type /FontDescriptor /FontName /ABCDEF+pdfceSyntheticBox "
            b"/Flags 4 /FontBBox [0 -200 1000 800] /ItalicAngle 0 "
            b"/Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 "
            b"/FontFile2 9 0 R >>"
        ),
        8: raw_stream(cid_to_gid, ""),
        9: raw_stream(ttf, f" /Length1 {len(ttf)}"),
        10: raw_stream(tounicode, ""),
    }
    return serialize(objects)





def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    path = OUT / "cidfonttype2-nocmap-embedded.pdf"
    path.write_bytes(cidfont_nocmap_embedded())
    print(f"wrote {path} ({path.stat().st_size} bytes)")

    p2 = OUT / "cidfonttype2-with-tounicode.pdf"
    p2.write_bytes(cidfont_with_tounicode())
    print(f"wrote {p2} ({p2.stat().st_size} bytes)  [injective /ToUnicode]")

    p3 = OUT / "cidfonttype2-noninjective-tounicode.pdf"
    p3.write_bytes(cidfont_noninjective_tounicode())
    print(f"wrote {p3} ({p3.stat().st_size} bytes)  [NON-injective /ToUnicode]")
    return 0


if __name__ == "__main__":
    sys.exit(main())
