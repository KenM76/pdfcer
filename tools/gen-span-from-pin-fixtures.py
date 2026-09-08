#!/usr/bin/env python3
"""Generate the synthetic `--span-from-pin` disambiguation fixture.

# What it is for

`EditRequest::spanning_from` (`Pass 272.0`) lets a caller say *"the run that
BEGINS at this operator"* — the combination a click-driven shell has and a
`find` string does not.

The fixture puts the same text on one page **twice**, in the two shapes that
matter, and nothing else:

    (AB) Tj (CD) Tj     "ABCD" SPANNING two show operators
    (ABCD) Tj           "ABCD" inside ONE show operator

★ **Both are required, and the pair is the test.** With only the spanning
occurrence, an implementation that ignored the pin entirely and simply scanned
the page would edit the right thing for the wrong reason and pass. With both,
`find = "ABCD"` is ambiguous on the page, so only a request that actually
consults the pin can pick a specific one — and pinning each in turn must edit
that one and leave the other untouched.

That is the real defect's shape, in miniature. Measured on the reporting
shell's own 36-sheet SolidWorks set: on the bill-of-materials sheet **122 runs**
have text that repeats, and `"1"` appears **108 times**. A quantity column is
close to the worst case for a page-scoped `find`.

# Byte spans, and why they are printed rather than assumed

A pin names an operator by its byte span in the *decoded content stream*. This
script prints the spans it built so the test can cite them, and asserts they
are what it thinks — a fixture whose spans silently moved would still edit
something, and the test would be asserting against the wrong operator without
saying so.

Helvetica, WinAnsi, no embedded program: this fixture is about operator
selection, and an embedded font would drag glyph availability into a test that
is not about it.

Usage:  python tools/gen-span-from-pin-fixtures.py
Output: fixtures/synthetic/text/span-from-pin.pdf
"""

from __future__ import annotations

import pathlib

OUT_DIR = pathlib.Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "text"

# Lines 1 and 3 span two operators each; line 2 holds the same text in one.
#
# ★★ THE THIRD LINE IS WHY THE SPAN SEARCH MUST START *AT* THE PIN rather than
# merely be enabled by it. With only lines 1 and 2, an implementation that
# turned the flag on and then scanned from operator 0 would still edit line 1
# -- which is what a pin on line 1 asked for, so it would pass. Line 3 is a
# SECOND spanning occurrence: pin it, and a scan-from-zero edits line 1
# instead. A sabotage removing the start-at-the-pin restriction stayed GREEN
# until this line existed.
CONTENT = (
    b"BT /F1 12 Tf 20 140 Td (AB) Tj (CD) Tj ET\n"
    b"BT /F1 12 Tf 20 100 Td (ABCD) Tj ET\n"
    b"BT /F1 12 Tf 20 60 Td (AB) Tj (CD) Tj ET\n"
)


def serialize(objects: dict[int, bytes]) -> bytes:
    """Classic xref layout, exactly-20-byte entries (§7.5.4)."""
    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets: dict[int, int] = {}
    highest = max(objects)
    for num in range(1, highest + 1):
        body = objects.get(num)
        if body is None:
            continue
        offsets[num] = len(out)
        out += f"{num} 0 obj\n".encode("ascii") + body + b"\nendobj\n"
    xref_at = len(out)
    out += f"xref\n0 {highest + 1}\n".encode("ascii") + b"0000000000 65535 f \n"
    for num in range(1, highest + 1):
        out += f"{offsets[num]:010d} 00000 n \n".encode("ascii")
    out += (
        f"trailer\n<< /Size {highest + 1} /Root 1 0 R >>\n"
        f"startxref\n{xref_at}\n%%EOF\n"
    ).encode("ascii")
    return bytes(out)


def span_of(token: bytes) -> tuple[int, int]:
    """`START:LEN` of `token` within the content stream, asserted unique.

    Unique because a pin that matched two places would make every assertion
    downstream ambiguous in exactly the way this fixture exists to study.
    """
    first = CONTENT.find(token)
    assert first >= 0, f"{token!r} not in the content stream"
    assert CONTENT.find(token, first + 1) == -1, f"{token!r} is not unique"
    return first, len(token)


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    objects = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        3: (
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] "
            b"/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
        ),
        4: b"<< /Length %d >>\nstream\n" % len(CONTENT) + CONTENT + b"\nendstream",
        5: (
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
            b"/Encoding /WinAnsiEncoding >>"
        ),
    }
    data = serialize(objects)
    (OUT_DIR / "span-from-pin.pdf").write_bytes(data)
    print(f"wrote span-from-pin.pdf ({len(data)} bytes) to {OUT_DIR}")
    # `(AB) Tj` appears twice, so each is located by a UNIQUE anchor (its
    # line's `Td`) and the operator's own offset added afterwards. Printing
    # the anchor's span instead would name the wrong bytes, and a pin built
    # from it would silently address the positioning operator.
    for label, anchor, token in (
        ("line 1 spanning, 1st op", b"20 140 Td ", b"(AB) Tj"),
        ("line 2 single operator", b"20 100 Td ", b"(ABCD) Tj"),
        ("line 3 spanning, 1st op", b"20 60 Td ", b"(AB) Tj"),
    ):
        anchor_start, anchor_len = span_of(anchor)
        start = anchor_start + anchor_len
        assert CONTENT[start : start + len(token)] == token, (
            f"{label}: expected {token!r} at {start}, found "
            f"{CONTENT[start : start + len(token)]!r}"
        )
        print(f"  {label:26s} {token.decode():11s} pin-span {start}:{len(token)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
