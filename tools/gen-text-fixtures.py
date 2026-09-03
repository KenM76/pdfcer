#!/usr/bin/env python3
"""Generate the synthetic PDF fixtures Pass 4 (text extraction) tests against.

WHY THIS EXISTS
---------------
`docs/LEGAL.md` §5 permits only synthetic or rights-cleared PDFs in
`fixtures/`. Every byte of every file this script writes is constructed
here, from nothing, so each fixture's provenance is this file alone.

Same discipline as the other `tools/gen-*-fixtures.py` generators: a
deliberately minimal writer with no PDF library behind it, so a fixture
cannot inherit a bug (or a helpful normalization) from the same code it
exists to test. Classic cross-reference table, exactly-20-byte entries
(ISO 32000-1 §7.5.4), no `/ID`, no timestamps — running this twice
produces byte-identical files.

WHAT EACH FIXTURE PROVES
------------------------
Each one isolates ONE claim from the §9.10 extraction contract, so a
failing test names the clause it broke rather than "extraction is wrong".

``simple-winansi.pdf``
    §9.10.2 **rung 2**. A standard-14 Helvetica with
    ``/Encoding /WinAnsiEncoding`` — the named-encoding disjunct of
    method 2's precondition, resolved through Annex D.2 and the Adobe
    Glyph List. Also carries a deliberately wide inter-word gap produced
    by a ``TJ`` offset with NO space glyph, which is the derived-space
    case (S3/S4), and a second line, which is the derived-line-break
    case (S5).

``identity-h-tounicode.pdf``
    §9.10.2 **rung 1** on the modern shape: ``/Type0`` +
    ``/Identity-H`` + a ``/ToUnicode`` CMap. The CMap deliberately
    exercises all three §9.10.3 forms in one file — a form-B ``bfrange``
    (last-byte increment), a form-C array with a **one-to-many ligature**
    destination, and a form-A ``bfchar`` with a **surrogate pair**.
    No font program is embedded: extraction does not need one, which is
    itself the point (rendering would refuse this file; extraction must
    not).

``identity-h-no-tounicode.pdf``
    **The headline honesty metric.** The same font shape with the
    ``/ToUnicode`` removed. §9.10.2 excludes ``Identity-H`` from rung 3
    by name and an ``Adobe-Identity-0`` descendant satisfies neither
    disjunct of rung 3's second test, so the ladder has nothing left:
    every code must fall through to the failure clause and be counted.
    A test that ever sees text come out of this file has found a
    fabrication.

``actual-text-drucker.pdf``
    §14.9.4's own EXAMPLE, verbatim in structure: the glyphs on the page
    read ``Dru`` / ``k-`` / ``ker`` and a ``/Span`` with
    ``/ActualText (c)`` covers the middle one, so the character content
    is **Drucker**. The clause's own gloss is the assertion.

``artifact-and-reversed.pdf``
    Two §14.8 mechanisms in one page: an ``/Artifact`` sequence with
    ``/Type /Pagination`` around a running head (classified, kept,
    excluded from plain text by policy — A1/A3), and a
    ``/ReversedChars`` sequence whose show strings hold their characters
    in reverse of page content order (§14.8.2.3.3), using the clause's
    own ``( olleH )`` / ``( . dlrow )`` example.

``tagged-marked.pdf``
    A ``/MarkInfo`` ``/Marked true`` catalog with ``/Suspects true`` and
    a ``/StructTreeRoot``, plus a ``/TagSuspect`` region. Nothing about
    the *text* is unusual; the point is the three document-level facts
    and the four named diagnostics they must produce.

USAGE
-----
    python tools/gen-text-fixtures.py

Re-run after changing anything here; the outputs are committed.
"""

from __future__ import annotations

import sys
from pathlib import Path

OUT_DIR = Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "text"

PAGE_WIDTH = 612
PAGE_HEIGHT = 792


# ---------------------------------------------------------------------------
# The minimal writer
# ---------------------------------------------------------------------------


def serialize(objects: dict[int, bytes], trailer_extra: str = "") -> bytes:
    """Lay out `objects` into a complete file with a classic xref table.

    §7.5.4's entry format is exactly 20 bytes: ten digits, a space, five
    digits, a space, the keyword, then a two-byte EOL. Written longhand
    so the byte count is visible at the call site.
    """
    out = bytearray(b"%PDF-1.7\n")
    # §7.5.2: a comment line with four bytes >= 128 marks the file binary.
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
        f"trailer\n<< /Size {highest + 1} /Root 1 0 R{trailer_extra} >>\n"
        f"startxref\n{xref_at}\n%%EOF\n"
    ).encode("ascii")
    return bytes(out)


def stream(body: bytes, extra: str = "") -> bytes:
    """An uncompressed stream object with a correct `/Length`.

    Uncompressed on purpose: a fixture whose failure mode could be "the
    filter is wrong" tests two things at once, and these fixtures are
    supposed to test exactly one.
    """
    return (
        f"<< /Length {len(body)}{extra} >>\nstream\n".encode("ascii")
        + body
        + b"\nendstream"
    )


def one_page_doc(
    content: bytes,
    fonts: str,
    extra_objects: dict[int, bytes],
    *,
    catalog_extra: str = "",
    properties: str = "",
) -> bytes:
    """Assemble a one-page document.

    Fixed object numbering so the tests can name objects directly:
    1 catalog, 2 root Pages node, 3 page, 4 content stream, 5+ whatever
    `extra_objects` supplies (fonts, CMaps, structure roots).
    """
    resources = f"<< /Font << {fonts} >>"
    if properties:
        resources += f" /Properties << {properties} >>"
    resources += " >>"

    objects: dict[int, bytes] = {
        1: f"<< /Type /Catalog /Pages 2 0 R{catalog_extra} >>".encode("ascii"),
        2: (
            f"<< /Type /Pages /Kids [3 0 R] /Count 1 "
            f"/MediaBox [0 0 {PAGE_WIDTH} {PAGE_HEIGHT}] "
            f"/Resources {resources} >>"
        ).encode("ascii"),
        3: b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>",
        4: stream(content),
    }
    objects.update(extra_objects)
    return serialize(objects)


# ---------------------------------------------------------------------------
# Fixture 1 — ladder rung 2, plus both derived-whitespace cases
# ---------------------------------------------------------------------------


def simple_winansi() -> bytes:
    """Standard-14 Helvetica, `/WinAnsiEncoding`.

    Line 1 uses a `TJ` array whose −2000 offset opens a gap between
    "Hello" and "world" with **no space glyph anywhere** — the S3/S4 case
    the derived-space heuristic exists for. Line 2 is a separate `Td`,
    which is the S5 derived-line-break case.
    """
    content = (
        b"BT\n"
        b"/F1 24 Tf\n"
        b"72 700 Td\n"
        b"[(Hello) -2000 (world)] TJ\n"
        b"0 -30 Td\n"
        b"(Second line) Tj\n"
        b"ET\n"
    )
    fonts = "/F1 5 0 R"
    extra = {
        5: (
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
            b"/Encoding /WinAnsiEncoding >>"
        )
    }
    return one_page_doc(content, fonts, extra)


# ---------------------------------------------------------------------------
# Fixtures 2 and 3 — the composite font, with and without /ToUnicode
# ---------------------------------------------------------------------------

# A ToUnicode CMap exercising all three §9.10.3 mapping forms.
#
#   form B: <0001>..<0005> -> U+0048 .. U+004C  ("HIJKL", last-byte
#           increment, which is the trap the clause spends a paragraph on)
#   form C: <0010>..<0012> -> an explicit array, the middle entry being a
#           THREE-code-point ligature destination (one-to-many)
#   form A: <0020>         -> a surrogate pair, U+2003E
TO_UNICODE_CMAP = b"""/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
/CMapName /Adobe-Identity-UCS def
/CMapType 2 def
1 begincodespacerange
<0000> <FFFF>
endcodespacerange
1 beginbfrange
<0001> <0005> <0048>
endbfrange
1 beginbfrange
<0010> <0012> [<0041> <00660066006C> <0042>]
endbfrange
1 beginbfchar
<0020> <D840DC3E>
endbfchar
endcmap
CMapName currentdict /CMap defineresource pop
end
end"""


def identity_h(with_to_unicode: bool) -> bytes:
    """`/Type0` + `/Identity-H` + `Adobe-Identity-0`, optionally with
    `/ToUnicode`.

    No font program is embedded. §9.7.5.2 forbids `Identity-H` with a
    non-embedded font and `pdfcer-render` correctly refuses such a file —
    but §9.10.2 rung 1 needs only the `/ToUnicode` entry, so extraction
    must succeed on exactly the file rendering rejects. Keeping the two
    apart is the reason extraction is a separate pipeline.

    The shown codes are 2-byte and cover all three CMap forms plus one
    code (`<0099>`) that is deliberately **outside** the CMap, so the
    per-code fallthrough (documented deviation 1) has something to fall
    through on even in the with-`/ToUnicode` file.
    """
    # <0001><0002><0003> -> "HIJ"; <0011> -> "ffl"; <0020> -> U+2003E;
    # <0099> -> uncovered.
    content = (
        b"BT\n"
        b"/F1 18 Tf\n"
        b"72 700 Td\n"
        b"<000100020003> Tj\n"
        b"<0011> Tj\n"
        b"<0020> Tj\n"
        b"<0099> Tj\n"
        b"ET\n"
    )
    font = (
        "<< /Type /Font /Subtype /Type0 /BaseFont /ABCDEF+TestCID "
        "/Encoding /Identity-H /DescendantFonts [6 0 R]"
    )
    if with_to_unicode:
        font += " /ToUnicode 7 0 R"
    font += " >>"

    extra: dict[int, bytes] = {
        5: font.encode("ascii"),
        6: (
            b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /ABCDEF+TestCID "
            b"/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> "
            b"/DW 1000 /W [1 [600 600 600] 16 18 700] "
            b"/CIDToGIDMap /Identity >>"
        ),
    }
    if with_to_unicode:
        extra[7] = stream(TO_UNICODE_CMAP)
    return one_page_doc(content, "/F1 5 0 R", extra)


# ---------------------------------------------------------------------------
# Fixture 4 — §14.9.4's own ActualText example
# ---------------------------------------------------------------------------


def actual_text_drucker() -> bytes:
    """§14.9.4's EXAMPLE, structurally verbatim.

    The clause's own listing is::

        (Dru) Tj
        /Span <</ActualText (c) >> BDC
        (k-) Tj
        EMC
        (ker) '

    with the gloss that the correct character content is ``Drucker`` —
    a German hyphenation that *changes the spelling*. Note that the `'`
    (next-line-and-show) sits OUTSIDE the `EMC`: reading the example as
    if the sequence covered it too yields ``Druc`` and loses ``ker``.

    The `/ActualText` is written **inline** rather than as a named
    `/Properties` resource, which §14.6.2 permits precisely because all
    of its values are direct objects.
    """
    content = (
        b"BT\n"
        b"/F1 24 Tf\n"
        b"24 TL\n"
        b"72 700 Td\n"
        b"(Dru) Tj\n"
        b"/Span << /ActualText (c) >> BDC\n"
        b"(k-) Tj\n"
        b"EMC\n"
        b"(ker) '\n"
        b"ET\n"
    )
    extra = {
        5: (
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
            b"/Encoding /WinAnsiEncoding >>"
        )
    }
    return one_page_doc(content, "/F1 5 0 R", extra)


# ---------------------------------------------------------------------------
# Fixture 5 — artifacts and ReversedChars
# ---------------------------------------------------------------------------


def artifact_and_reversed() -> bytes:
    """An `/Artifact` running head plus §14.8.2.3.3's `ReversedChars`
    example.

    The artifact carries a full Table 330 property list
    (`/Type /Pagination /Subtype /Header /BBox [...]`) because NOTE 1
    asks writers to supply one whenever possible, and because a fixture
    with only the bare `/Artifact BMC` form would not exercise the
    classification path at all. The bare form appears too, on the folio.

    The `ReversedChars` block is the clause's own example verbatim —
    `( olleH )` then `( . dlrow )`, which "represents the text
    `Hello world .`". Reversing the *sequence* instead of each *string*
    is the classic bug; this fixture catches it, because the wrong
    implementation produces `. dlrowolleH` reversed as a whole.
    """
    content = (
        b"/Artifact << /Type /Pagination /Subtype /Header "
        b"/BBox [72 750 540 780] >> BDC\n"
        b"BT /F1 10 Tf 72 760 Td (Running head) Tj ET\n"
        b"EMC\n"
        b"BT /F1 24 Tf 72 700 Td (Real content) Tj ET\n"
        b"/ReversedChars BMC\n"
        b"BT /F1 24 Tf 72 650 Td ( olleH ) Tj 200 0 Td ( . dlrow ) Tj ET\n"
        b"EMC\n"
        b"/Artifact BMC\n"
        b"BT /F1 10 Tf 300 40 Td (1) Tj ET\n"
        b"EMC\n"
    )
    extra = {
        5: (
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
            b"/Encoding /WinAnsiEncoding >>"
        )
    }
    return one_page_doc(content, "/F1 5 0 R", extra)


# ---------------------------------------------------------------------------
# Fixture 6 — the three document-level facts
# ---------------------------------------------------------------------------


def tagged_marked() -> bytes:
    """`/MarkInfo` `/Marked true` + `/Suspects true` + `/StructTreeRoot`,
    and a `/TagSuspect` region in the content.

    §14.8.1 makes the four Tagged-PDF guarantees (every code mappable,
    word breaks explicit, artifacts distinguished, appearance order)
    conditional on `Marked true`; §14.8.2.3.1 makes `Suspects true` the
    producer's own disclaimer of its ordering. Both are document-level
    facts an extractor must read and report, and neither changes a single
    character of the extracted text — which is exactly why they need a
    fixture of their own rather than being asserted incidentally.

    The `/StructTreeRoot` here is deliberately minimal: this Pass
    *detects* it and names the deferral; it does not traverse it.
    """
    content = (
        b"BT /F1 18 Tf 72 700 Td (Marked content here) Tj ET\n"
        b"/TagSuspect << /TagSuspect /Ordering >> BDC\n"
        b"BT /F1 18 Tf 72 660 Td (Suspect region) Tj ET\n"
        b"EMC\n"
        b"/P << /MCID 0 >> BDC\n"
        b"BT /F1 18 Tf 72 620 Td (Inside MCID zero) Tj ET\n"
        b"EMC\n"
    )
    extra = {
        5: (
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
            b"/Encoding /WinAnsiEncoding >>"
        ),
        6: b"<< /Type /StructTreeRoot /K [] >>",
    }
    return one_page_doc(
        content,
        "/F1 5 0 R",
        extra,
        catalog_extra=(
            " /StructTreeRoot 6 0 R "
            "/MarkInfo << /Marked true /Suspects true >>"
        ),
    )


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    fixtures = {
        "simple-winansi.pdf": simple_winansi(),
        "identity-h-tounicode.pdf": identity_h(with_to_unicode=True),
        "identity-h-no-tounicode.pdf": identity_h(with_to_unicode=False),
        "actual-text-drucker.pdf": actual_text_drucker(),
        "artifact-and-reversed.pdf": artifact_and_reversed(),
        "tagged-marked.pdf": tagged_marked(),
    }
    for name, data in fixtures.items():
        path = OUT_DIR / name
        path.write_bytes(data)
        print(f"wrote {path} - {len(data)} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
