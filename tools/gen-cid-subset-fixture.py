"""Generate fixtures/synthetic/text/cid-shaped-subset.pdf and its variants
(Pass 430.2).

A composite font as Word and Chrome emit one: `/Type0`, `/Identity-H`, a
`/CIDFontType2` descendant with `/CIDToGIDMap /Identity` (so CID = GID), a
two-byte `/ToUnicode`, and a `/W` covering only the shown CIDs. The program
is `gen-word-subset-fixture.py`'s, so its glyph ids are:

  2 A, 3 B, 4 C   shown on page 1 (page 2 shows A), /W 2 [667 600 722]
  5 D             in /ToUnicode, outlined, never shown -> typeable, /W grows
  6 E             in /ToUnicode, EMPTY slot             -> refused
  8 Delta, 9 Zhe  outlined, not in /ToUnicode           -> typeable, map grows

`-shared-descendant` draws page 2 through a second Type0 font sharing the
descendant, so its `/W` cannot be rewritten. Synthetic; no real font bytes.
"""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("gen_word", HERE / "gen-word-subset-fixture.py")
word = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(word)
gen = word.gen

BASE_FONT = "CIDSUB+pdfceCidShape"

TO_UNICODE = (
    b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n"
    b"/CMapName /Adobe-Identity-UCS def /CMapType 2 def\n"
    b"1 begincodespacerange <0000> <FFFF> endcodespacerange\n"
    b"1 beginbfrange <0002> <0004> <0041> endbfrange\n"
    b"2 beginbfchar <0005> <0044> <0006> <0045> endbfchar\n"
    b"endcmap CMapName currentdict /CMap defineresource pop end end\n"
)


def type0(base_font: str) -> bytes:
    return (
        f"<< /Type /Font /Subtype /Type0 /BaseFont /{base_font} /Encoding /Identity-H "
        f"/DescendantFonts [6 0 R] /ToUnicode 9 0 R >>"
    ).encode("ascii")


def build_pdf(shared: bool = False) -> bytes:
    ttf = word.build_program()
    page1 = b"BT\n/F0 24 Tf\n72 600 Td\n<000200030004> Tj\nET\n"
    page2 = b"BT\n/F0 24 Tf\n72 600 Td\n<0002> Tj\nET\n"
    objects = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: (
            f"<< /Type /Pages /Kids [3 0 R 10 0 R] /Count 2 "
            f"/MediaBox [0 0 {gen.PAGE_WIDTH} {gen.PAGE_HEIGHT}] "
            f"/Resources << /Font << /F0 5 0 R >> >> >>"
        ).encode("ascii"),
        3: b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>",
        4: gen.raw_stream(page1, ""),
        5: type0(BASE_FONT),
        6: (
            f"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{BASE_FONT} "
            f"/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> "
            f"/FontDescriptor 7 0 R /CIDToGIDMap /Identity /DW 1000 "
            f"/W [2 [{word.widths('ABC')}]] >>"
        ).encode("ascii"),
        7: (
            f"<< /Type /FontDescriptor /FontName /{BASE_FONT} "
            f"/Flags 32 /FontBBox [-1361 -665 4096 2060] /ItalicAngle 0 "
            f"/Ascent 905 /Descent -212 /CapHeight 716 /StemV 80 "
            f"/FontFile2 8 0 R >>"
        ).encode("ascii"),
        8: gen.raw_stream(ttf, f" /Length1 {len(ttf)}"),
        9: gen.raw_stream(TO_UNICODE, ""),
        10: b"<< /Type /Page /Parent 2 0 R /Contents 11 0 R >>",
        11: gen.raw_stream(page2, ""),
    }
    if shared:
        objects[10] = (
            b"<< /Type /Page /Parent 2 0 R /Contents 11 0 R "
            b"/Resources << /Font << /F0 12 0 R >> >> >>"
        )
        objects[12] = type0("Twin")
    return gen.serialize(objects)


VARIANTS = (
    ("cid-shaped-subset.pdf", False),
    ("cid-shaped-subset-shared-descendant.pdf", True),
)


def main() -> int:
    out_dir = Path(sys.argv[1]) if len(sys.argv) > 1 else gen.OUT
    for name, shared in VARIANTS:
        path = out_dir / name
        path.write_bytes(build_pdf(shared))
        print(f"wrote {path} ({path.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
