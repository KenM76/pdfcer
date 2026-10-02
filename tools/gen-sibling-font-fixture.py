"""Generate fixtures/synthetic/text/sibling-font*.pdf (decision 174).

The measured shape: a run in a one-glyph composite subset beside a simple
TrueType subset of the same face on the same page. Both embed
`gen-word-subset-fixture.py`'s program (glyph ids 2 A, 3 B, 4 C); /F0's copy
maps only A in its `cmap`, so the program itself cannot supply B and the
decision 172 extension refuses before the sibling is tried.

  /F0  Type0 Identity-H, CIDFontType2, /BaseFont SIBAAA+pdfceSib,
       /ToUnicode maps only <0002> -> A; shows "AA"
  /F1  TrueType WinAnsi, /BaseFont SIBBBB+pdfceSib; shows "ABC"

`-other-face` names /F1 `SIBBBB+pdfceOther`, so it is not a sibling.
Synthetic; no real font bytes.
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

TO_UNICODE = (
    b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n"
    b"/CMapName /Adobe-Identity-UCS def /CMapType 2 def\n"
    b"1 begincodespacerange <0000> <FFFF> endcodespacerange\n"
    b"1 beginbfchar <0002> <0041> endbfchar\n"
    b"endcmap CMapName currentdict /CMap defineresource pop end end\n"
)


def descriptor(base_font: str, program: int) -> bytes:
    return (
        f"<< /Type /FontDescriptor /FontName /{base_font} "
        f"/Flags 32 /FontBBox [-1361 -665 4096 2060] /ItalicAngle 0 "
        f"/Ascent 905 /Descent -212 /CapHeight 716 /StemV 80 "
        f"/FontFile2 {program} 0 R >>"
    ).encode("ascii")


def a_only(ttf: bytes) -> bytes:
    """`ttf` with its `cmap` cut down to A."""
    import io

    from fontTools.ttLib import TTFont

    font = TTFont(io.BytesIO(ttf))
    for table in font["cmap"].tables:
        table.cmap = {u: g for u, g in table.cmap.items() if u == 0x41}
    out = io.BytesIO()
    font.save(out)
    return out.getvalue()


def build_pdf(sibling_stem: str) -> bytes:
    ttf = word.build_program(False)
    subset = a_only(ttf)
    content = (
        b"BT\n/F0 24 Tf\n72 600 Td\n<00020002> Tj\nET\n"
        b"BT\n/F1 24 Tf\n72 500 Td\n(ABC) Tj\nET\n"
    )
    simple = f"SIBBBB+{sibling_stem}"
    objects = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: (
            f"<< /Type /Pages /Kids [3 0 R] /Count 1 "
            f"/MediaBox [0 0 {gen.PAGE_WIDTH} {gen.PAGE_HEIGHT}] >>"
        ).encode("ascii"),
        3: b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R "
        b"/Resources << /Font << /F0 5 0 R /F1 10 0 R >> >> >>",
        4: gen.raw_stream(content, ""),
        5: b"<< /Type /Font /Subtype /Type0 /BaseFont /SIBAAA+pdfceSib /Encoding /Identity-H "
        b"/DescendantFonts [6 0 R] /ToUnicode 9 0 R >>",
        6: (
            f"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /SIBAAA+pdfceSib "
            f"/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> "
            f"/FontDescriptor 7 0 R /CIDToGIDMap /Identity /DW 1000 "
            f"/W [2 [{word.widths('A')}]] >>"
        ).encode("ascii"),
        7: descriptor("SIBAAA+pdfceSib", 12),
        8: gen.raw_stream(ttf, f" /Length1 {len(ttf)}"),
        9: gen.raw_stream(TO_UNICODE, ""),
        10: (
            f"<< /Type /Font /Subtype /TrueType /BaseFont /{simple} "
            f"/FirstChar 65 /LastChar 67 /Widths [{word.widths('ABC')}] "
            f"/Encoding /WinAnsiEncoding /FontDescriptor 11 0 R >>"
        ).encode("ascii"),
        11: descriptor(simple, 8),
        12: gen.raw_stream(subset, f" /Length1 {len(subset)}"),
    }
    return gen.serialize(objects)


VARIANTS = (
    ("sibling-font.pdf", "pdfceSib"),
    ("sibling-font-other-face.pdf", "pdfceOther"),
)


def main() -> int:
    out_dir = Path(sys.argv[1]) if len(sys.argv) > 1 else gen.OUT
    for name, stem in VARIANTS:
        path = out_dir / name
        path.write_bytes(build_pdf(stem))
        print(f"wrote {path} ({path.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
