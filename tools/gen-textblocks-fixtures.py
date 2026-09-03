#!/usr/bin/env python3
"""Generate the synthetic fixture Pass 14.0 (editable text model + block
recognition) tests against.

WHY THIS EXISTS
---------------
`docs/LEGAL.md` §5 permits only synthetic or rights-cleared PDFs in
`fixtures/`. Every byte of the file this script writes is constructed
here, from nothing, so the fixture's provenance is this file alone. Same
discipline as the sibling `tools/gen-*-fixtures.py` generators: a
deliberately minimal writer with no PDF library behind it, so the fixture
cannot inherit a bug (or a helpful normalisation) from the same code it
exists to test. Classic cross-reference table, exactly-20-byte entries
(ISO 32000-1 §7.5.4), no `/ID`, no timestamps — running this twice
produces byte-identical files.

WHAT THE FIXTURE PROVES
-----------------------
``multi-column.pdf``
    A two-column, four-paragraph, ten-line page in standard-14 Helvetica
    (`/WinAnsiEncoding`), NO embedded font program (extraction needs
    none — §9.10.2 rung 2). It exercises the whole Run -> Line -> Column
    -> Block recognition pipeline of `pdfcer_core::text_edit`:

      * ten lines, each a separate `BT ... Td (text) Tj ET` object, so the
        derived line breaks (§14.8 S5) come out at exact, known baselines;
      * two x-bands 250 units apart (left column at x=72, right at x=322),
        content emitted left-column-first then right, so the fixture also
        proves the left-to-right band ORDERING is by geometry, not content
        order (§14.8.2.3.1);
      * within each column, a paragraph-sized leading gap (28 units, twice
        the 14-unit line leading) that must segment two paragraphs
        (§14.8 S9), for four paragraphs total;
      * the second left-column paragraph is painted in blue (`0 0 1 rg`)
        and the rest in the default black, so the PROVENANCE fill-colour
        capture (`TextColor::Rgb` vs the unset default) has something to
        record when extracted with `capture_provenance`.

    Nothing here is tagged: the point is that all of the above structure
    is DERIVED and must be recognised from geometry alone.

USAGE
-----
    python tools/gen-textblocks-fixtures.py

Re-run after changing anything here; the output is committed.
"""

from __future__ import annotations

import sys
from pathlib import Path

OUT_DIR = Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "textblocks"

PAGE_WIDTH = 612
PAGE_HEIGHT = 792


def serialize(objects: dict[int, bytes], trailer_extra: str = "") -> bytes:
    """Lay out `objects` into a complete file with a classic xref table.

    §7.5.4's entry format is exactly 20 bytes: ten digits, a space, five
    digits, a space, the keyword, then a two-byte EOL. Written longhand so
    the byte count is visible at the call site.
    """
    out = bytearray(b"%PDF-1.7\n")
    out += b"%\xe2\xe3\xcf\xd3\n"  # §7.5.2 binary marker

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
    """An uncompressed stream object with a correct `/Length`."""
    return (
        f"<< /Length {len(body)}{extra} >>\nstream\n".encode("ascii")
        + body
        + b"\nendstream"
    )


def text_line(x: int, y: int, text: str) -> bytes:
    """One text line as its own text object at an absolute origin.

    Each line is a separate `BT ... ET` with an absolute `Td`, so the
    glyph baselines are exactly the `y` values chosen here and the derived
    line breaks fall precisely where the recognition test expects.
    """
    return f"BT /F1 10 Tf {x} {y} Td ({text}) Tj ET\n".encode("ascii")


def multi_column() -> bytes:
    """Two columns, four paragraphs, ten lines (see the module docstring)."""
    lx, rx = 72, 322  # left/right column origins, 250 units apart

    body = bytearray()

    # --- Left column, paragraph 1 (black), three lines at 14pt leading ---
    body += text_line(lx, 740, "Left column paragraph one")
    body += text_line(lx, 726, "runs across three lines")
    body += text_line(lx, 712, "of ordinary body text.")
    # --- Left column, paragraph 2 (BLUE), after a 28pt gap, two lines ---
    body += b"0 0 1 rg\n"  # DeviceRGB blue fill (§8.6.4.3) -> provenance
    body += text_line(lx, 684, "Left column paragraph two")
    body += text_line(lx, 670, "has two short lines.")
    body += b"0 g\n"  # back to black (§8.6.4.2)

    # --- Right column, paragraph 1 (black), two lines ---
    body += text_line(rx, 740, "Right column paragraph one")
    body += text_line(rx, 726, "spans just two lines here.")
    # --- Right column, paragraph 2 (black), after a 28pt gap, three lines --
    body += text_line(rx, 698, "Right column paragraph two")
    body += text_line(rx, 684, "is a three line block")
    body += text_line(rx, 670, "closing the sample page.")

    resources = "<< /Font << /F1 5 0 R >> >>"
    objects: dict[int, bytes] = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: (
            f"<< /Type /Pages /Kids [3 0 R] /Count 1 "
            f"/MediaBox [0 0 {PAGE_WIDTH} {PAGE_HEIGHT}] "
            f"/Resources {resources} >>"
        ).encode("ascii"),
        3: b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>",
        4: stream(bytes(body)),
        5: (
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
            b"/Encoding /WinAnsiEncoding >>"
        ),
    }
    return serialize(objects)


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    fixtures = {"multi-column.pdf": multi_column()}
    for name, data in fixtures.items():
        path = OUT_DIR / name
        path.write_bytes(data)
        print(f"wrote {path} - {len(data)} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
