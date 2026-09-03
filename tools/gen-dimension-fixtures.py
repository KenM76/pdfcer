#!/usr/bin/env python3
"""Generate the Pass 12.M2 dimensioning **base-geometry** fixtures.

These are 100% synthetic, hand-authored one-page PDFs carrying only drawn
geometry (a line, a short arc built from small line segments, two shapes in
two regions). They are the INPUT geometry the dimensioning tools measure —
NOT dimensioned files. The *dimensioned* fixtures are produced by running
`pdfcer` on these bases; the exact commands are recorded in
`fixtures/synthetic/dimension/PROVENANCE.md` and re-run by the CI fixture step.

Fixture-sourcing rule (project rule 7 / LEGAL §5): fixtures are synthetic or
clearly rights-cleared only. Every byte here is emitted by this script.

Usage:
    python tools/gen-dimension-fixtures.py
"""

from __future__ import annotations

import math
import pathlib

OUT = pathlib.Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "dimension"


def build_pdf(content: bytes, media: str = "[0 0 400 400]") -> bytes:
    """Assemble a minimal one-page PDF whose page /Contents is `content`."""
    objects: list[bytes] = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        (
            b"<< /Type /Page /Parent 2 0 R /MediaBox "
            + media.encode()
            + b" /Resources << >> /Contents 4 0 R >>"
        ),
        b"<< /Length " + str(len(content)).encode() + b" >>\nstream\n" + content + b"\nendstream",
    ]
    buf = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets = []
    for i, body in enumerate(objects):
        offsets.append(len(buf))
        buf += f"{i + 1} 0 obj\n".encode() + body + b"\nendobj\n"
    xref_at = len(buf)
    size = len(objects) + 1
    buf += f"xref\n0 {size}\n0000000000 65535 f \n".encode()
    for off in offsets:
        buf += f"{off:010} 00000 n \n".encode()
    buf += (
        f"trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n".encode()
    )
    return bytes(buf)


def linear_base() -> bytes:
    """A single horizontal stroked line from (100,200) to (300,200) — 200 pt."""
    return build_pdf(b"1 w 100 200 m 300 200 l S")


def short_arc_base() -> bytes:
    """A SHORT arc (40 deg) of a circle centred (200,200) r=100, drawn as
    12 small straight line segments — the operator's stated 'small line
    segments approximating an arc' case. Taubin recovers the radius
    near-unbiased where Kåsa (algebraic) biases it low; proven by the
    `dimension::fit::tests::taubin_beats_kasa_on_short_arcs` headless test.
    The points are exactly on the circle so the CLI fit residual is ~0."""
    cx, cy, r = 200.0, 200.0, 100.0
    sweep = math.radians(40.0)
    n = 12
    pts = [
        (cx + r * math.cos(sweep * i / (n - 1)), cy + r * math.sin(sweep * i / (n - 1)))
        for i in range(n)
    ]
    ops = [f"{pts[0][0]:.4f} {pts[0][1]:.4f} m"]
    ops += [f"{x:.4f} {y:.4f} l" for (x, y) in pts[1:]]
    ops.append("S")
    return build_pdf(("1 w " + " ".join(ops)).encode())


def two_region_base() -> bytes:
    """Two lines in two page regions — the substrate for a two-group,
    different-scale page (each line dimensioned into its own group)."""
    left = "0.5 w 50 300 m 150 300 l S"
    right = "0.5 w 250 80 m 350 80 l S"
    return build_pdf((left + " " + right).encode())


def plain_base() -> bytes:
    """A blank page — the substrate for the feet-inches / OCG-toggle /
    PieceInfo-mirror authored fixtures (geometry is not required to author a
    dimension by explicit coordinates)."""
    return build_pdf(b"")


FIXTURES = {
    "linear-base.pdf": linear_base,
    "short-arc-base.pdf": short_arc_base,
    "two-region-base.pdf": two_region_base,
    "plain-base.pdf": plain_base,
}


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    for name, fn in FIXTURES.items():
        (OUT / name).write_bytes(fn())
        print(f"wrote {OUT / name}")


if __name__ == "__main__":
    main()
