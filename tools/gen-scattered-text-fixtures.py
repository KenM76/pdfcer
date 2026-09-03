#!/usr/bin/env python3
"""Generate the SCATTERED-TEXT hit-test fixture (selection root-cause repro).

WHY THIS EXISTS
===============
A SolidWorks-exported drawing was unselectable in the Obj tool: clicking a
visible line selected nothing useful, and sometimes drew a selection box
"that doesn't seem to correspond to anything".

Measured cause (`object-list --hit` on the real export, which is NOT in this
repo and never will be — LEGAL.md §5):

    hit at=1041,216 index=5871 kind=text candidates=1
    hit-candidate index=5871 kind=text bbox=23.1,14.1,1564.3,1216.5

ONE text object carried every dimension label on the sheet, because the
exporter emits them all inside a single `BT`...`ET` block. pdfcer unions the
per-run boxes into one `page_bbox`, so that object's bounds span the whole
drawing. Text hit-tests by bounding box, and the object is painted late, so
it sits on top of everything and swallows every click.

WHAT THIS FIXTURE REPRODUCES
============================
The same shape, synthetically: ONE text object with two short runs placed at
opposite corners of the page. Their union covers the page, while the ink
covers almost none of it. A click in the empty middle therefore "hits" the
text object even though nothing is drawn there — which is the entire defect,
reduced to two `Td`s.

Deliberately synthetic (LEGAL.md §5). The operator's drawing is proprietary
work product and stays out of the repository; only the STRUCTURE it revealed
is reproduced here, from first principles.

The horizontal rule between the two runs is the control: it is real ink in
the empty middle, so the correct post-fix behaviour is that clicking there
hits the PATH and not the text. Without it the fixture could only prove the
false positive, not that the true positive survives the fix.

Usage:
    python tools/gen-scattered-text-fixtures.py [OUT_DIR]
"""

from __future__ import annotations

import sys
from pathlib import Path

PAGE_W, PAGE_H = 612, 792
OUT = Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "text"


def serialize(objects: dict[int, bytes]) -> bytes:
    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    highest = max(objects)
    offsets: dict[int, int] = {}
    for num in range(1, highest + 1):
        body = objects.get(num)
        if body is None:
            continue
        offsets[num] = len(out)
        out += f"{num} 0 obj\n".encode("ascii") + body + b"\nendobj\n"
    xref_at = len(out)
    out += f"xref\n0 {highest + 1}\n".encode("ascii")
    out += b"0000000000 65535 f \n"
    for num in range(1, highest + 1):
        out += (
            f"{offsets[num]:010d} 00000 n \n".encode("ascii")
            if num in offsets
            else b"0000000000 65535 f \n"
        )
    out += (
        f"trailer\n<< /Size {highest + 1} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n"
    ).encode("ascii")
    return bytes(out)


def raw_stream(body: bytes) -> bytes:
    return f"<< /Length {len(body)} >>\nstream\n".encode("ascii") + body + b"\nendstream"


def scattered_text() -> bytes:
    # ONE BT..ET, two runs at opposite corners. The union of their per-run
    # boxes spans the page; the ink does not. Painted LAST so it is topmost
    # in paint order, exactly as the exporter's label block was.
    content = (
        b"q 1 w 0 0 0 RG\n"
        b"72 396 m 540 396 l S\n"          # the control: real ink, mid-page
        b"Q\n"
        b"BT\n/F1 10 Tf\n"
        b"1 0 0 1 72 740 Tm\n(TOP LEFT LABEL) Tj\n"
        b"1 0 0 1 430 60 Tm\n(BOTTOM RIGHT) Tj\n"
        b"ET\n"
    )
    objects: dict[int, bytes] = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: (
            f"<< /Type /Pages /Kids [3 0 R] /Count 1 "
            f"/MediaBox [0 0 {PAGE_W} {PAGE_H}] "
            f"/Resources << /Font << /F1 5 0 R >> >> >>"
        ).encode("ascii"),
        3: b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>",
        4: raw_stream(content),
        5: (
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
            b"/Encoding /WinAnsiEncoding >>"
        ),
    }
    return serialize(objects)


def main() -> int:
    out_dir = Path(sys.argv[1]) if len(sys.argv) > 1 else OUT
    out_dir.mkdir(parents=True, exist_ok=True)
    p = out_dir / "scattered-text-one-object.pdf"
    p.write_bytes(scattered_text())
    print(f"wrote {p} ({p.stat().st_size} bytes)")
    print("  one BT..ET, two runs at opposite corners + a mid-page rule")
    print("  probe: --hit 306,300  (empty space; must NOT hit the text)")
    print("  control: --hit 306,396 (on the rule; must hit the PATH)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
