#!/usr/bin/env python3
"""Regenerate fixtures/synthetic/brotli/ — a page whose content stream is
compressed with `/BrotliDecode`.

WHY THIS EXISTS
---------------
`/BrotliDecode` is a **PDF Association extension** (EXTN-BROTLI-1 v1.3,
2026-08-19), not part of ISO 32000-2:2020, so no conformance corpus contains
a file that uses it. pdfcer's unit tests prove the decoder round-trips bytes;
only a real PDF proves the decoder is *reachable from a page*.

That gap is not hypothetical in this project. A Pass shipped earlier the same
week was very nearly dead code for exactly this reason — correct arithmetic
that no content stream could reach — and it was caught only because a fixture
was built to exercise it.

WHAT THE FIXTURE IS
-------------------
One page, one content stream, `/Filter /BrotliDecode`, drawing two filled
rectangles at known coordinates. The drawing is deliberately trivial: the
thing under test is the *filter*, and a complex page would make a decode
failure and a rendering failure look alike.

`brotli-with-predictor.pdf` is the same page with a PNG-Up predictor
(`/Predictor 12 /Columns 4`) applied before compression. It exists because
the extension retitles Table 8 to include Brotli — the predictors apply
**verbatim**, sharing `FlateDecode`'s code — and because **pdf.js silently
ignores `/DecodeParms` predictors on Brotli** while honouring them for Flate
and LZW. A file that renders correctly in pdfcer and wrongly in pdf.js is
worth having on disk, and the divergence is pdf.js's.

★ THE ONE THING THIS FIXTURE MAY NOT CONTAIN: an inline image using
`/BrotliDecode`. EXTN-BROTLI-1 §5.2 forbids it outright, and pdfcer refuses it
(`ImageCodecError::BrotliNotAllowedInline`). Writing one here would mean
shipping a file that violates the very extension the fixture exists to
demonstrate.

PROVENANCE
----------
Authored here byte by byte from ISO 32000-1's object syntax; the compressed
payload is produced by the reference Brotli encoder via Python's `brotli`
module. No third-party PDF is copied or adapted (`docs/LEGAL.md` §5).

This script is NOT part of the build and is not a workspace member.

USAGE
-----
    python tools/gen-brotli-fixture.py
"""

from __future__ import annotations

import pathlib

import brotli

OUT = pathlib.Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "brotli"

# ★ THE LENGTH IS A MULTIPLE OF `/Columns` (4), AND THAT IS DELIBERATE.
#
# A PNG predictor works on fixed-width rows, so a payload whose length is not
# a multiple of `/Columns` has to be padded — and a decoder cannot know the
# padding was not data, so it hands the padding back. The first draft of this
# fixture was 75 bytes; the predictor variant then decoded to 76, and the test
# asserting both documents carry identical content failed on a trailing zero.
#
# That failure was the FIXTURE's, not pdfcer's, and the fix belongs here rather
# than in a weakened assertion: relaxing the test to "identical apart from
# padding" would have thrown away its ability to catch a real predictor bug.
# The extra trailing newline is a no-op in a content stream and brings the
# length to 76 = 4 × 19.
CONTENT = (
    b"1 0 0 RG 0.9 0.2 0.2 rg\n"
    b"20 20 120 80 re f\n"
    b"0.2 0.4 0.9 rg\n"
    b"60 60 120 80 re f\n"
    b"\n"
)
assert len(CONTENT) % 4 == 0, "see the note above — padding would break this fixture's own claim"


def png_up_encode(data: bytes, columns: int) -> bytes:
    """Apply the PNG "Up" predictor (tag 2) that `/Predictor 12` selects.

    Each output row is one tag byte followed by `columns` bytes, each the
    difference from the byte directly above it. This is the *encoder*; pdfcer's
    `filters::predictor` is the decoder and the two must be exact inverses,
    which is what the fixture checks end to end.
    """
    if len(data) % columns:
        data = data + b"\x00" * (columns - len(data) % columns)
    out = bytearray()
    prev = bytes(columns)
    for i in range(0, len(data), columns):
        row = data[i : i + columns]
        out.append(2)  # PNG filter type: Up
        out.extend(((row[j] - prev[j]) & 0xFF) for j in range(columns))
        prev = row
    return bytes(out)


def build(predictor: bool) -> bytes:
    """Assemble the one-page document. Offsets are accumulated as objects are
    emitted, so the xref is built from the same list as the body."""
    payload = png_up_encode(CONTENT, 4) if predictor else CONTENT
    stream = brotli.compress(payload, quality=11)

    objects: list[bytes] = []

    def add(body: bytes) -> int:
        objects.append(body)
        return len(objects)

    catalog = add(b"<< /Type /Catalog /Pages 2 0 R >>")
    pages = add(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
    page = add(
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 160] "
        b"/Resources << >> /Contents 4 0 R >>"
    )
    parms = (
        b" /DecodeParms << /Predictor 12 /Columns 4 >>" if predictor else b""
    )
    content = add(
        b"<< /Filter /BrotliDecode" + parms +
        b" /Length " + str(len(stream)).encode() + b" >>\nstream\n"
        + stream
        + b"\nendstream"
    )
    assert (catalog, pages, page, content) == (1, 2, 3, 4), (
        "object numbering drifted; the /Pages, /Parent and /Contents "
        "references above are written as literals"
    )

    out = bytearray(b"%PDF-2.0\n%\xe2\xe3\xcf\xd3\n")
    offsets = [0]
    for i, body in enumerate(objects, start=1):
        offsets.append(len(out))
        out += str(i).encode() + b" 0 obj\n" + body + b"\nendobj\n"

    startxref = len(out)
    n = len(objects) + 1
    out += b"xref\n0 " + str(n).encode() + b"\n0000000000 65535 f \n"
    for off in offsets[1:]:
        out += f"{off:010d} 00000 n \n".encode()
    out += (
        b"trailer\n<< /Size " + str(n).encode() + b" /Root 1 0 R >>\n"
        b"startxref\n" + str(startxref).encode() + b"\n%%EOF\n"
    )
    return bytes(out)


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    for name, pred in (("brotli-content.pdf", False), ("brotli-with-predictor.pdf", True)):
        data = build(pred)
        (OUT / name).write_bytes(data)
        print(f"wrote {OUT / name}  ({len(data)} bytes)")


if __name__ == "__main__":
    main()
