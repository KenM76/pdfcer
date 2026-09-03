#!/usr/bin/env python3
"""Generate the operator-supplied-fonts demo/test fixture (decision 012).

Emits ONE synthetic, self-contained PDF that references a NON-embedded
simple TrueType font named ``Calibri`` — a font pdfcer does not bundle —
so that:

  * without ``--font-dir`` it renders with a bundled Base-14 substitute
    (disclosed ``substituted=`` / ``supplied=0``), and
  * with ``--font-dir`` pointing at a folder holding a ``Calibri.*`` face
    it renders from that supplied face (disclosed ``supplied=`` /
    ``substituted=0``), with IDENTICAL glyph positions (the advances come
    from the PDF's own ``/Widths``, decision 004 §3.6).

The font dictionary carries ``/Flags 32`` (Nonsymbolic) and an explicit
``/Widths`` array so the advance is face-independent and the letters
resolve through StandardEncoding in any Latin substitute face. There is
NO ``/FontFile*`` — the program is deliberately absent, which is the
whole point of the fixture.

This is a SYNTHETIC fixture (docs/LEGAL.md §5): every byte is generated
here, nothing is copied from a real-world document.

Usage:
    python tools/gen-supplied-font-fixtures.py [OUT_DIR]
        OUT_DIR defaults to fixtures/synthetic/.
"""

from __future__ import annotations

import sys
from pathlib import Path

# The shown string. Every code is a Latin letter present in
# StandardEncoding and in every bundled/supplied Latin face.
TEXT = b"Calibri HI"
# One 600-unit advance per code (glyph space, 1000 = 1 text-space unit),
# so the pen advance is deterministic and face-independent.
FIRST_CHAR = 32
LAST_CHAR = 126
WIDTHS = [600] * (LAST_CHAR - FIRST_CHAR + 1)


def build_pdf() -> bytes:
    """Assemble a one-page PDF with a classic (§7.5.4) cross-reference
    table. Object 1 = catalog, 2 = pages, 3 = page, 4 = contents,
    5 = the non-embedded /Calibri font dictionary."""
    widths = " ".join(str(w) for w in WIDTHS)
    content = b"BT /F1 40 Tf 40 700 Td (" + TEXT + b") Tj ET"

    objects: list[bytes] = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
        b"/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        b"<< /Length " + str(len(content)).encode() + b" >>\nstream\n"
        + content + b"\nendstream",
        # The star of the fixture: a non-embedded simple TrueType named
        # Calibri. No /FontFile2 — the program is intentionally absent.
        b"<< /Type /Font /Subtype /TrueType /BaseFont /Calibri "
        b"/FirstChar " + str(FIRST_CHAR).encode()
        + b" /LastChar " + str(LAST_CHAR).encode()
        + b" /Widths [" + widths.encode() + b"] "
        b"/FontDescriptor << /Type /FontDescriptor /FontName /Calibri "
        b"/Flags 32 /ItalicAngle 0 /Ascent 750 /Descent -250 "
        b"/CapHeight 700 /StemV 80 /FontBBox [-100 -250 1100 750] >> >>",
    ]

    buf = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets: list[int] = []
    for i, body in enumerate(objects, start=1):
        offsets.append(len(buf))
        buf += f"{i} 0 obj\n".encode() + body + b"\nendobj\n"

    xref_at = len(buf)
    n = len(objects) + 1
    buf += f"xref\n0 {n}\n".encode()
    buf += b"0000000000 65535 f \n"
    for off in offsets:
        buf += f"{off:010d} 00000 n \n".encode()
    buf += (
        f"trailer\n<< /Size {n} /Root 1 0 R >>\n"
        f"startxref\n{xref_at}\n%%EOF\n"
    ).encode()
    return bytes(buf)


def main() -> int:
    out_dir = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("fixtures/synthetic")
    out_dir.mkdir(parents=True, exist_ok=True)
    path = out_dir / "nonembedded_calibri.pdf"
    path.write_bytes(build_pdf())
    print(f"wrote {path} ({path.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
