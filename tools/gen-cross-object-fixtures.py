#!/usr/bin/env python3
"""Generate the synthetic Word-shaped cross-object text-edit fixture.

Word writes each fragment of a visual line as its own text object, inside its
own `q ... Q` and its own `/Span <</MCID n>> BDC ... EMC`, positioned with an
absolute `Tm`, and ends the line with a space-only object. A label such as
"Driver-Side" becomes three objects. This fixture reproduces that shape with
Helvetica (WinAnsi, not embedded) so the test is about operator structure, not
glyph availability.

Lines (top to bottom):

    y=700  "Driver" "-" "Side" " "      the three-object label + trailing space
    y=660  "Driver" "-" "Side" " "      the same label again (pin must choose)
    y=620  "Left" "-Hand Door"          a match ending mid-object (tail)
    y=580  "Front" "Panel"              second object uses a DIFFERENT font
                                        resource (/F2) -> a font seam
    y=540  "Driver-Side"                one object: the unpinned control

Every fragment's x is the previous fragment's x plus its Helvetica advance, so
the joined line reads exactly as one run would.

Usage:  python tools/gen-cross-object-fixtures.py
Output: fixtures/synthetic/text/cross-object-word.pdf
"""

from __future__ import annotations

import pathlib

OUT_DIR = pathlib.Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "text"

# Helvetica advance widths (AFM, per 1000 em) for the characters used.
WIDTHS = {
    " ": 278, "-": 333, "D": 722, "r": 333, "i": 222, "v": 500, "e": 556,
    "S": 667, "d": 556, "L": 556, "f": 278, "t": 278, "H": 722, "a": 556,
    "n": 556, "o": 556, "F": 611, "P": 667, "l": 222,
}
SIZE = 12.0


def advance(text: str) -> float:
    return sum(WIDTHS[c] for c in text) * SIZE / 1000.0


def fmt(v: float) -> str:
    s = f"{v:.3f}".rstrip("0").rstrip(".")
    return s or "0"


def line(y: float, frags: list[tuple[str, str]], mcid: int) -> tuple[bytes, int]:
    out = b""
    x = 72.0
    for font, text in frags:
        out += (
            f"/Span <</MCID {mcid}>> BDC q BT /{font} 12 Tf "
            f"1 0 0 1 {fmt(x)} {fmt(y)} Tm ({text}) Tj ET Q EMC\n"
        ).encode("ascii")
        x += advance(text)
        mcid += 1
    return out, mcid


def build_content() -> bytes:
    content = b""
    mcid = 0
    for y, frags in (
        (700, [("F1", "Driver"), ("F1", "-"), ("F1", "Side"), ("F1", " ")]),
        (660, [("F1", "Driver"), ("F1", "-"), ("F1", "Side"), ("F1", " ")]),
        (620, [("F1", "Left"), ("F1", "-Hand Door")]),
        (580, [("F1", "Front"), ("F2", "Panel")]),
        (540, [("F1", "Driver-Side")]),
    ):
        chunk, mcid = line(y, frags, mcid)
        content += chunk
    return content


CONTENT = build_content()


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


def pin_of(anchor: bytes, token: bytes) -> tuple[int, int]:
    """Span of the first `token` after the unique `anchor`."""
    at = CONTENT.find(anchor)
    assert at >= 0 and CONTENT.find(anchor, at + 1) == -1, f"{anchor!r} not unique"
    start = CONTENT.find(token, at)
    assert start >= 0, f"{token!r} not after {anchor!r}"
    return start, len(token)


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    helv = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
    objects = {
        1: b"<< /Type /Catalog /Pages 2 0 R /MarkInfo << /Marked true >> >>",
        2: b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        3: (
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 760] "
            b"/Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> /Contents 4 0 R >>"
        ),
        4: b"<< /Length %d >>\nstream\n" % len(CONTENT) + CONTENT + b"\nendstream",
        5: helv,
        6: helv,
    }
    data = serialize(objects)
    (OUT_DIR / "cross-object-word.pdf").write_bytes(data)
    print(f"wrote cross-object-word.pdf ({len(data)} bytes) to {OUT_DIR}")
    for label, anchor, token in (
        ("line 700 first object", b"72 700 Tm", b"(Driver) Tj"),
        ("line 660 first object", b"72 660 Tm", b"(Driver) Tj"),
        ("line 620 first object", b"72 620 Tm", b"(Left) Tj"),
        ("line 580 first object", b"72 580 Tm", b"(Front) Tj"),
        ("line 540 single object", b"72 540 Tm", b"(Driver-Side) Tj"),
    ):
        start, length = pin_of(anchor, token)
        print(f"  {label:24s} pin-span {start}:{length}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
