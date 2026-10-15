#!/usr/bin/env python3
"""Generate fixtures/synthetic/reflow/block_mixed_looks.pdf: one two-line
paragraph mixing three looks, for EditSession::edit_block_text keeping each
character's look (pdfcer-gui request G199).

Every byte is constructed here (LEGAL.md §5 category (a)); the classic-xref
writer is the one in gen-reflow-fixtures.py. Helvetica and Helvetica-Bold,
WinAnsi, no font program embedded. Deterministic.

Line 1: "Plain words then " (F1, black) "bold" (F2 Helvetica-Bold, black)
" and " (F1, black) "red" (F1, red) " words" (F1, black).
Line 2: "on a second line." (F1, black).

    python tools/gen-block-mixed-looks-fixture.py
"""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("gen_reflow", HERE / "gen-reflow-fixtures.py")
base = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(base)

OUT = HERE.parent / "fixtures" / "synthetic" / "reflow" / "block_mixed_looks.pdf"

CONTENT = (
    b"BT /F1 11 Tf 1 0 0 1 72 700 Tm 0 g (Plain words then ) Tj "
    b"/F2 11 Tf (bold) Tj /F1 11 Tf ( and ) Tj 1 0 0 rg (red) Tj 0 g ( words) Tj ET\n"
    b"BT /F1 11 Tf 1 0 0 1 72 686 Tm 0 g (on a second line.) Tj ET\n"
)


def build() -> bytes:
    objects: dict[int, bytes] = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: b"<< /Type /Pages /Kids [3 0 R] /Count 1 "
        b"/Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> >>",
        3: b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>",
        4: base.stream(CONTENT),
        5: b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
        6: b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold "
        b"/Encoding /WinAnsiEncoding >>",
    }
    return base.serialize(objects)


def main() -> int:
    data = build()
    OUT.write_bytes(data)
    print(f"wrote {OUT} - {len(data)} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
