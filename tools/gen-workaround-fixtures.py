#!/usr/bin/env python3
"""Generate the synthetic text-edit workaround fixtures (decision 175).

Each file holds a run the exact edit refuses and a workaround can edit:

    workaround-quote.pdf      a ' and a " operator (rewrite as T* + Tj)
    workaround-seam.pdf       "Hello" drawn as "Hel" in Helvetica and "lo" in
                              Times-Roman, then " world" continuing from it
                              (retype, with the follower held in place)
    workaround-composite.pdf  /F1 a vertical (Identity-V) Type0 font with a
                              ToUnicode map, /F2 a horizontal Type0 font with
                              none (retype in the fallback face)

No font program is embedded; every font is a standard 14 name or a
non-embedded CIDFont, so the tests are about operator structure.

Usage:  python tools/gen-workaround-fixtures.py
Output: fixtures/synthetic/text/workaround-*.pdf
"""

from __future__ import annotations

import pathlib

OUT_DIR = pathlib.Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "text"

HELV = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
TIMES = b"<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman /Encoding /WinAnsiEncoding >>"

QUOTE = b"BT /F1 12 Tf 14 TL 72 700 Td (Title) Tj (Quoted line) ' (after) Tj 2 1 (Double line) \" ET\n"
SEAM = b"BT /F1 12 Tf 72 700 Td (Hel) Tj /F2 12 Tf (lo) Tj /F1 12 Tf ( world) Tj ET\n"
COMPOSITE = (
    b"BT /F1 12 Tf 100 700 Td <00410042> Tj ET\n"
    b"BT /F2 12 Tf 72 600 Td <00430044> Tj ET\n"
)

TOUNICODE = (
    b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n"
    b"/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n"
    b"/CMapName /Adobe-Identity-UCS def /CMapType 2 def\n"
    b"1 begincodespacerange <0000> <FFFF> endcodespacerange\n"
    b"2 beginbfchar <0041> <0041> <0042> <0042> endbfchar\n"
    b"endcmap CMapName currentdict /CMap defineresource pop end end\n"
)


def stream(data: bytes) -> bytes:
    return b"<< /Length %d >>\nstream\n" % len(data) + data + b"\nendstream"


def serialize(objects: dict[int, bytes]) -> bytes:
    """Classic xref layout, exactly-20-byte entries (ISO 32000-1 7.5.4)."""
    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets: dict[int, int] = {}
    highest = max(objects)
    for num in range(1, highest + 1):
        offsets[num] = len(out)
        out += f"{num} 0 obj\n".encode("ascii") + objects[num] + b"\nendobj\n"
    xref_at = len(out)
    out += f"xref\n0 {highest + 1}\n".encode("ascii") + b"0000000000 65535 f \n"
    for num in range(1, highest + 1):
        out += f"{offsets[num]:010d} 00000 n \n".encode("ascii")
    out += (
        f"trailer\n<< /Size {highest + 1} /Root 1 0 R >>\n"
        f"startxref\n{xref_at}\n%%EOF\n"
    ).encode("ascii")
    return bytes(out)


def page(content: bytes, fonts: dict[str, bytes]) -> dict[int, bytes]:
    """Catalog, pages, one page; fonts become objects 5, 6, ..."""
    refs = " ".join(f"/{k} {5 + i} 0 R" for i, k in enumerate(fonts))
    objects = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        3: (
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 760] "
            b"/Resources << /Font << " + refs.encode("ascii") + b" >> >> /Contents 4 0 R >>"
        ),
        4: stream(content),
    }
    for i, body in enumerate(fonts.values()):
        objects[5 + i] = body
    return objects


def cid_font(first: int, encoding: bytes, to_unicode: int | None) -> dict[int, bytes]:
    """A /Type0 font at `first`, its CIDFontType2 at first+1, descriptor at first+2."""
    tu = b" /ToUnicode %d 0 R" % to_unicode if to_unicode else b""
    return {
        first: (
            b"<< /Type /Font /Subtype /Type0 /BaseFont /pdfcerCid /Encoding /" + encoding
            + b" /DescendantFonts [%d 0 R]" % (first + 1) + tu + b" >>"
        ),
        first + 1: (
            b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /pdfcerCid "
            b"/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> "
            b"/FontDescriptor %d 0 R /DW 1000 /CIDToGIDMap /Identity >>" % (first + 2)
        ),
        first + 2: (
            b"<< /Type /FontDescriptor /FontName /pdfcerCid /Flags 32 "
            b"/FontBBox [0 -200 1000 800] /ItalicAngle 0 /Ascent 800 /Descent -200 "
            b"/CapHeight 700 /StemV 80 >>"
        ),
    }


def composite() -> dict[int, bytes]:
    objects = page(COMPOSITE, {"F1": b"", "F2": b""})
    objects.update(cid_font(5, b"Identity-V", 11))
    # /F1 -> 5 (vertical, ToUnicode), /F2 -> 8 (horizontal, no ToUnicode).
    objects[3] = objects[3].replace(b"/F2 6 0 R", b"/F2 8 0 R")
    objects.update(cid_font(8, b"Identity-H", None))
    objects[11] = stream(TOUNICODE)
    return objects


def pin(content: bytes, token: bytes) -> str:
    at = content.find(token)
    assert at >= 0 and content.find(token, at + 1) == -1, token
    return f"{at}:{len(token)}"


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    for name, objects in (
        ("workaround-quote.pdf", page(QUOTE, {"F1": HELV})),
        ("workaround-seam.pdf", page(SEAM, {"F1": HELV, "F2": TIMES})),
        ("workaround-composite.pdf", composite()),
    ):
        data = serialize(objects)
        (OUT_DIR / name).write_bytes(data)
        print(f"wrote {name} ({len(data)} bytes)")
    print(f"  quote ' pin-span {pin(QUOTE, b'(Quoted line) ' + bytes([39]))}")
    print(f"  composite /F1 pin-span {pin(COMPOSITE, b'<00410042> Tj')}")
    print(f"  composite /F2 pin-span {pin(COMPOSITE, b'<00430044> Tj')}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
