#!/usr/bin/env python3
"""Generate fixtures/synthetic/reflow/fidelity.pdf: blocks whose re-wrap must
keep each word's own font, size, kerning and colour (Pass 432.0).

Every byte is constructed here (LEGAL.md §5 category (a)); the classic-xref
writer is the one in gen-reflow-fixtures.py. No font program is embedded.
Deterministic: two runs produce identical bytes.

The block under test is the page's first, so its recognised index is 0.
Pages:

1. composite: an Identity-H Type0 font with /W and /ToUnicode, a flipped
   0.75 CTM and a `1 0 0 -1 x y Tm` per line, each line its own BT ... ET
   with 2-byte hex `Tj` (the shape a browser's print-to-PDF writes).
2. styles: Helvetica / Helvetica-Bold / Helvetica-Oblique runs inside one
   paragraph.
3. kerning: a TJ-kerned first word that a narrower wrap does not move.
4. sizes: one 14 pt word in a 10 pt paragraph.
5. colour: red words mid-line and last, the colour reset only after the
   last show, then a black paragraph after the block.
6. path: an `re f` between two lines of the block (refused by name).
7. scale: two lines under different text-matrix scales (refused by name).
8. word-justified: Courier lines flush to x=72..300 by a per-line `Tw`, the
   last line at `0 Tw`.
9. letter-justified: the same by a per-line `Tc`.
10. bullets: a WinAnsi bullet at x=72, its text moved to the x=84 hanging
    indent by `Td`, a continuation line at 84, a second item and a closing
    paragraph.
11. numbered: `1.` and its text in one string with a space glyph, the
    continuation at the 83.12 indent that string reaches, then a `2.` item.

    python tools/gen-reflow-fidelity-fixtures.py
"""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("gen_reflow", HERE / "gen-reflow-fixtures.py")
base = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(base)

OUT = HERE.parent / "fixtures" / "synthetic" / "reflow" / "fidelity.pdf"

COMPOSITE_LINES = [
    "Synthetic words fill this short",
    "paragraph so that a narrower wrap",
    "moves them onto more lines.",
]
SPACE_CID = 3


def cid_table() -> dict[str, int]:
    """A code per distinct character, space first at 3 as subsetters assign."""
    chars = sorted({c for line in COMPOSITE_LINES for c in line} - {" "})
    table = {" ": SPACE_CID}
    for i, c in enumerate(chars):
        table[c] = 4 + i
    return table


def to_unicode(table: dict[str, int]) -> bytes:
    entries = "".join(f"<{cid:04X}> <{ord(c):04X}>\n" for c, cid in sorted(table.items(), key=lambda kv: kv[1]))
    return (
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n"
        "/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n"
        "/CMapName /Adobe-Identity-UCS def /CMapType 2 def\n"
        "1 begincodespacerange <0000> <FFFF> endcodespacerange\n"
        f"{len(table)} beginbfchar\n{entries}endbfchar\n"
        "endcmap CMapName currentdict /CMap defineresource pop end end"
    ).encode("ascii")


def composite_page(table: dict[str, int]) -> bytes:
    out = bytearray(b"q .75 0 0 -.75 0 792 cm 0 g\n")
    y = 96
    for line in COMPOSITE_LINES:
        hexs = "".join(f"{table[c]:04X}" for c in line)
        out += f"BT /F4 16 Tf 1 0 0 -1 96 {y} Tm <{hexs}> Tj ET\n".encode("ascii")
        y += 18
    out += b"Q\n"
    return bytes(out)


def composite_fonts(table: dict[str, int], first: int) -> dict[int, bytes]:
    """Type0 at `first`, descendant, descriptor and ToUnicode after it."""
    widths = " ".join(f"{cid} [{278 if c in ' .' else 500}]" for c, cid in sorted(table.items(), key=lambda kv: kv[1]))
    name = "SYNTHA+Helvetica"
    return {
        first: (
            f"<< /Type /Font /Subtype /Type0 /BaseFont /{name} /Encoding /Identity-H "
            f"/DescendantFonts [{first + 1} 0 R] /ToUnicode {first + 3} 0 R >>"
        ).encode("ascii"),
        first + 1: (
            f"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{name} "
            "/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> "
            f"/FontDescriptor {first + 2} 0 R /DW 500 /W [{widths}] /CIDToGIDMap /Identity >>"
        ).encode("ascii"),
        first + 2: (
            f"<< /Type /FontDescriptor /FontName /{name} /Flags 32 /FontBBox [0 -200 1000 900] "
            "/ItalicAngle 0 /Ascent 900 /Descent -200 /CapHeight 700 /StemV 80 >>"
        ).encode("ascii"),
        first + 3: base.stream(to_unicode(table)),
    }


STYLES = (
    b"BT /F1 10 Tf 72 740 Td (Plain words with ) Tj /F2 10 Tf (bold) Tj "
    b"/F1 10 Tf ( and ) Tj /F3 10 Tf (italic) Tj /F1 10 Tf ( runs inside) Tj ET\n"
    b"BT /F1 10 Tf 72 726 Td (one paragraph, then ) Tj /F2 10 Tf (strong) Tj "
    b"/F1 10 Tf ( words) Tj ET\n"
    b"BT /F1 10 Tf 72 712 Td (close the ) Tj /F3 10 Tf (block) Tj /F1 10 Tf (.) Tj ET\n"
)

KERNING = (
    b"BT /F1 12 Tf 72 740 Td [(A) 80 (V) 80 (A) 60 (T) 40 (AR)] TJ "
    b"( leads a line that a narrower) Tj ET\n"
    b"BT /F1 12 Tf 72 725 Td (wrap breaks onto more lines.) Tj ET\n"
)

SIZES = (
    b"BT /F1 10 Tf 72 740 Td (Small text then ) Tj /F1 14 Tf (LARGE) Tj "
    b"/F1 10 Tf ( words and more) Tj ET\n"
    b"BT /F1 10 Tf 72 726 Td (small words close the block.) Tj ET\n"
)

COLOUR = (
    b"BT /F1 10 Tf 72 740 Td (Plain then ) Tj 1 0 0 rg (red) Tj 0 g "
    b"( words again here) Tj ET\n"
    b"BT /F1 10 Tf 72 726 Td (and the block ends in ) Tj 1 0 0 rg (red.) Tj 0 g ET\n"
    b"BT /F1 10 Tf 72 500 Td (after) Tj ET\n"
)

PATH = (
    b"BT /F1 10 Tf 72 740 Td (A rule sits between) Tj ET\n"
    b"72 734 100 0.5 re f\n"
    b"BT /F1 10 Tf 72 726 Td (these two lines.) Tj ET\n"
)

SCALE = (
    b"BT /F1 10 Tf 72 740 Td (One scale on this line) Tj ET\n"
    b"BT /F1 5 Tf 2 0 0 2 72 726 Tm (another on this one) Tj ET\n"
)


# Courier at 10 pt advances 6 pt per code, so a line of L codes and k spaces
# reaches the 228 pt measure with Tw = (228 - 6L) / k or Tc = (228 - 6L) / L.
MEASURE = 228.0
WORD_JUSTIFIED = [
    "Each full line here is stretched by",
    "its own word spacing so both edges",
    "meet the margins while the last one",
]
LETTER_JUSTIFIED = [
    "Letter spacing widens every line",
    "so its glyphs reach the margin",
]


BULLETS = (
    b"BT /F1 10 Tf 72 740 Td (\x95) Tj 12 0 Td (First item text that runs long enough to) Tj ET\n"
    b"BT /F1 10 Tf 84 727 Td (wrap onto a second line at the indent.) Tj ET\n"
    b"BT /F1 10 Tf 72 660 Td (\x95) Tj 12 0 Td (Second item.) Tj ET\n"
    b"BT /F1 10 Tf 72 647 Td (A closing paragraph after the list.) Tj ET\n"
)

# Helvetica "1. " advances 556 + 278 + 278 units: 11.12 pt at 10 pt.
NUMBERED = (
    b"BT /F1 10 Tf 72 740 Td (1. Numbered item text that runs long enough) Tj ET\n"
    b"BT /F1 10 Tf 83.12 727 Td (to wrap at its hanging indent.) Tj ET\n"
    b"BT /F1 10 Tf 72 660 Td (2. Next item.) Tj ET\n"
)


def justified(lines: list[str], last: str, op: str) -> bytes:
    out = bytearray()
    y = 740
    for t in lines:
        room = MEASURE - 6 * len(t)
        value = room / t.count(" ") if op == "Tw" else room / len(t)
        out += f"BT /F5 10 Tf {value:.4f} {op} 72 {y} Td ({t}) Tj ET\n".encode("ascii")
        y -= 13
    out += f"BT /F5 10 Tf 0 {op} 72 {y} Td ({last}) Tj ET\n".encode("ascii")
    return bytes(out)


def build() -> bytes:
    table = cid_table()
    contents = [
        composite_page(table),
        STYLES,
        KERNING,
        SIZES,
        COLOUR,
        PATH,
        SCALE,
        justified(WORD_JUSTIFIED, "stays ragged.", "Tw"),
        justified(LETTER_JUSTIFIED, "but the last one stays tight.", "Tc"),
        BULLETS,
        NUMBERED,
    ]
    font_first = 3 + 2 * len(contents)
    fonts = {
        "F1": b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
        "F2": b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>",
        "F3": b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Oblique /Encoding /WinAnsiEncoding >>",
        "F5": b"<< /Type /Font /Subtype /Type1 /BaseFont /Courier /Encoding /WinAnsiEncoding >>",
    }
    objects: dict[int, bytes] = {1: b"<< /Type /Catalog /Pages 2 0 R >>"}
    font_refs = []
    num = font_first
    for key, body in fonts.items():
        objects[num] = body
        font_refs.append(f"/{key} {num} 0 R")
        num += 1
    objects.update(composite_fonts(table, num))
    font_refs.append(f"/F4 {num} 0 R")
    kids = []
    for i, body in enumerate(contents):
        page, content = 3 + 2 * i, 4 + 2 * i
        kids.append(f"{page} 0 R")
        objects[page] = (
            f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {content} 0 R >>"
        ).encode("ascii")
        objects[content] = base.stream(body)
    objects[2] = (
        f"<< /Type /Pages /Kids [{' '.join(kids)}] /Count {len(kids)} "
        f"/Resources << /Font << {' '.join(font_refs)} >> >> >>"
    ).encode("ascii")
    return base.serialize(objects)


def main() -> int:
    data = build()
    OUT.write_bytes(data)
    print(f"wrote {OUT} - {len(data)} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
