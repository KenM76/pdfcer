#!/usr/bin/env python3
"""Generate the synthetic fixture Pass 15.0 (within-block reflow ENGINE +
alignment auto-detect, READ-ONLY) demonstrates and is tested against.

WHY THIS EXISTS
---------------
`docs/LEGAL.md` §5 permits only synthetic or rights-cleared PDFs in
`fixtures/`. Every byte of the file this script writes is constructed
here, from nothing, so the fixture's provenance is this file alone. Same
discipline as the sibling `tools/gen-*-fixtures.py` generators: a
deliberately minimal writer with no PDF library behind it. Classic
cross-reference table, exactly-20-byte entries (ISO 32000-1 §7.5.4), no
`/ID`, no timestamps — running this twice produces byte-identical files.

WHY COURIER (monospace)
-----------------------
The alignment auto-detect and greedy re-wrap are geometry: they read glyph
x-positions and advances. Courier is a standard-14 **monospace** font — every
glyph, including the space, advances 600/1000 em = exactly 6.0 pt at size 10.
That makes every line's left edge, right edge and midpoint an exact,
hand-computable number, so a right/centre/justified paragraph can be laid out
flush to an exact margin with no font-metric table in this script. No embedded
font program is needed (extraction uses §9.10.2 rung 2; positions come from
pdfcer's own Courier metrics), so the fixture is legally clean (§5 category (a),
wholly synthetic) and reproducible.

WHAT THE FIXTURE PROVES (one file, five pages)
----------------------------------------------
``reflow.pdf``
  * page 1 — a **LEFT**-aligned paragraph: all left edges flush at x=72,
    right edges ragged. Detect => Left.
  * page 2 — a **RIGHT**-aligned paragraph: all right edges flush at x=300,
    left edges ragged. Detect => Right.
  * page 3 — a **CENTRE**-aligned paragraph: all midpoints flush at x=200,
    both edges ragged. Detect => Center.
  * page 4 — a **JUSTIFIED** paragraph: three body lines flush BOTH margins
    (x=72 .. x=300, exactly 38 chars each), the last line short. Detect =>
    Justified.
  * page 5 — a small page (200 x 120) whose 2-line block, when re-wrapped at a
    narrowed width, grows many lines downward PAST the page bottom (cropbox) —
    so the overflow disclosure (decision 015 §3.5 / R76) is COMPUTED. It is a
    LEFT block (flush left, ragged right) at default width.

Each page holds exactly one paragraph, so the recognised block index is 0.
The CLI reflow-preview path recognises with first-line-indent splitting
relaxed (a right/centre/justified paragraph's ragged left edges would
otherwise fragment it), so each page is one column, one block.

USAGE
-----
    python tools/gen-reflow-fixtures.py

Re-run after changing anything here; the output is committed.
"""

from __future__ import annotations

import sys
from pathlib import Path

OUT_DIR = Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "reflow"

# Courier advance at size 10: 600/1000 * 10 = 6.0 pt per glyph (incl. space).
ADV = 6.0
SIZE = 10


def serialize(objects: dict[int, bytes]) -> bytes:
    """Lay out `objects` into a complete file with a classic xref table.

    §7.5.4's entry format is exactly 20 bytes: ten digits, a space, five
    digits, a space, the keyword, then a two-byte EOL.
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
        f"trailer\n<< /Size {highest + 1} /Root 1 0 R >>\n"
        f"startxref\n{xref_at}\n%%EOF\n"
    ).encode("ascii")
    return bytes(out)


def stream(body: bytes) -> bytes:
    """An uncompressed stream object with a correct `/Length`."""
    return (
        f"<< /Length {len(body)} >>\nstream\n".encode("ascii") + body + b"\nendstream"
    )


def text_line(x: float, y: float, text: str) -> bytes:
    """One text line as its own text object at an absolute origin.

    Each line is a separate `BT ... ET` with an absolute `Td`, so the glyph
    baselines are exactly the `y` values chosen here and the derived line
    breaks fall precisely where the recogniser expects. `x` may be a float
    (right/centre alignment needs sub-integer origins occasionally); it is
    formatted compactly.
    """
    xs = f"{x:g}"
    ys = f"{y:g}"
    # Escape the two content-string metacharacters that can appear in prose.
    esc = text.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")
    return f"BT /F1 {SIZE} Tf {xs} {ys} Td ({esc}) Tj ET\n".encode("ascii")


def width_of(text: str) -> float:
    """Rendered width of `text` in Courier at size 10 (mono: 6 pt/char)."""
    return ADV * len(text)


def left_page() -> bytes:
    """LEFT-aligned: all lines flush at x=72, ragged right."""
    x = 72
    lines = [
        "The quick brown fox jumps",  # 25 -> right 222
        "over the lazy dog and then",  # 26 -> right 228
        "runs",  # 4  -> right 96
        "away today.",  # 11 -> right 138
    ]
    body = bytearray()
    y = 740
    for t in lines:
        body += text_line(x, y, t)
        y -= 14
    return bytes(body)


def right_page() -> bytes:
    """RIGHT-aligned: all right edges flush at x=300, ragged left."""
    margin = 300
    lines = [
        "The quick brown fox jumps",  # 25
        "over the lazy dog",  # 17
        "runs away",  # 9
        "now.",  # 4
    ]
    body = bytearray()
    y = 740
    for t in lines:
        x = margin - width_of(t)
        body += text_line(x, y, t)
        y -= 14
    return bytes(body)


def center_page() -> bytes:
    """CENTRE-aligned: all midpoints flush at x=200, both edges ragged."""
    center = 200
    lines = [
        "The quick brown fox jumps",  # 25
        "over the lazy dog",  # 17
        "runs away fast",  # 14
        "now.",  # 4
    ]
    body = bytearray()
    y = 740
    for t in lines:
        x = center - width_of(t) / 2.0
        body += text_line(x, y, t)
        y -= 14
    return bytes(body)


def justified_page() -> bytes:
    """JUSTIFIED: body lines flush BOTH margins (x=72..300, 38 chars), last
    line short.

    A monospace justified line is achieved by making each body line exactly
    38 characters (38 * 6 = 228 pt = the 72..300 measure). Padded with
    trailing spaces where the prose falls short — trailing spaces still
    advance 6 pt each, so the right edge lands exactly on x=300; the reflow
    tokeniser collapses them.
    """
    x = 72
    body_len = 38  # 38 * 6 = 228 pt -> right edge 72 + 228 = 300
    bodies = [
        "Justified paragraphs reach both margins",
        "with every full line flush left and right",
        "while the last line stays at the base one",
    ]
    last = "The end."  # short last line -> ragged right (not stretched)
    body = bytearray()
    y = 740
    for t in bodies:
        padded = (t[:body_len]).ljust(body_len)
        body += text_line(x, y, padded)
        y -= 14
    body += text_line(x, y, last)
    return bytes(body)


def overflow_page() -> bytes:
    """A LEFT 2-line block near the bottom of a small (200x120) page. At
    default width it fits; narrowed it re-wraps into many lines that grow
    downward past the page bottom (the overflow disclosure)."""
    x = 20
    body = bytearray()
    body += text_line(x, 40, "aa aa aa aa aa aa")  # 17 -> right 122
    body += text_line(x, 26, "bb bb bb bb")  # 11 -> right 86 (ragged)
    return bytes(body)


def build() -> bytes:
    resources = "<< /Font << /F1 13 0 R >> >>"
    big_mediabox = "/MediaBox [0 0 612 792]"
    small_mediabox = "/MediaBox [0 0 200 120]"

    objects: dict[int, bytes] = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: (
            f"<< /Type /Pages /Kids [3 0 R 5 0 R 7 0 R 9 0 R 11 0 R] /Count 5 "
            f"/Resources {resources} >>"
        ).encode("ascii"),
        3: f"<< /Type /Page /Parent 2 0 R {big_mediabox} /Contents 4 0 R >>".encode("ascii"),
        4: stream(left_page()),
        5: f"<< /Type /Page /Parent 2 0 R {big_mediabox} /Contents 6 0 R >>".encode("ascii"),
        6: stream(right_page()),
        7: f"<< /Type /Page /Parent 2 0 R {big_mediabox} /Contents 8 0 R >>".encode("ascii"),
        8: stream(center_page()),
        9: f"<< /Type /Page /Parent 2 0 R {big_mediabox} /Contents 10 0 R >>".encode("ascii"),
        10: stream(justified_page()),
        11: f"<< /Type /Page /Parent 2 0 R {small_mediabox} /Contents 12 0 R >>".encode("ascii"),
        12: stream(overflow_page()),
        13: (
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Courier "
            b"/Encoding /WinAnsiEncoding >>"
        ),
    }
    return serialize(objects)


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    data = build()
    path = OUT_DIR / "reflow.pdf"
    path.write_bytes(data)
    print(f"wrote {path} - {len(data)} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
