#!/usr/bin/env python3
"""Generate the synthetic fixtures Pass 16.0 (add NEW page text / FF-D) tests
against.

WHY THIS EXISTS
---------------
`docs/LEGAL.md` §5 permits only synthetic or rights-cleared PDFs in
`fixtures/`. Every structural byte of these files is constructed here, from
nothing, by a deliberately minimal writer with no PDF library behind it
(same discipline as the sibling `tools/gen-*-fixtures.py`). Classic
cross-reference table, exactly-20-byte entries (ISO 32000-1 §7.5.4), no
`/ID`, no timestamps -> running this twice produces byte-identical files.

All fonts are NON-embedded named Standard-14 simple fonts (no `/FontFile`);
Pass 16.0 never embeds a glyph program (R79), and neither do these fixtures,
so the whole corpus is wholly synthetic placeholder content (LEGAL.md §5
category (a), CC0-equivalent) with NO third-party font bytes.

THE FIXTURES
------------
``plain.pdf``
    One page, a SINGLE `/Contents` stream, the page's OWN `/Resources`
    (`/Font << /F1 … >>`). Content: one run "Original page text". Proves:
    adding text leaves the original content stream byte-identical (the output
    is an incremental append whose prefix is the input); the new run renders
    and re-extracts; a `/Font` entry is merged into the page's own resources.
    Also the missing-glyph refusal fixture (add a non-WinAnsi character).

``inherited-resources.pdf``
    TWO sibling pages (3, 4) that BOTH OMIT `/Resources` and therefore INHERIT
    the `/Pages` node's `/Resources` (`/Font << /F1 5 0 R >>`) — the shared
    ancestor. Proves the §7.7.3.4 inheritance trap: adding text to page 0 gives
    that page its OWN `/Resources` (referencing the same `/F1` font object)
    plus the new font, and does NOT mutate the shared `/Pages` `/Resources`
    (so sibling page 1 is untouched).

``tagged.pdf``
    One page whose run sits inside `/P << /MCID 0 >> BDC … EMC`, with the
    catalog carrying `/MarkInfo << /Marked true >>` and a minimal
    `/StructTreeRoot`. Proves R73: a new run added to a tagged page is emitted
    as UNTAGGED page content and that fact is disclosed (no structure element
    is fabricated).

``certified-locked.pdf``
    ``plain.pdf`` PLUS an ENFORCED certification (DocMDP) signature: the
    catalog carries `/Perms << /DocMDP 6 0 R >>` and object 6 is a signature
    dictionary (`/Type /Sig`, `/ByteRange`) whose `/Reference` is a `/DocMDP`
    transform with `/TransformParams << /P 1 >>` (ISO 32000-1 §12.8.4 Table
    258 / Table 254 P=1 — "no changes permitted"). Proves the add-text
    certification guard: because `/Perms → /DocMDP` is present, enforcement is
    a `shall`, so adding page content (point OR box) is REFUSED with the same
    `CertificationForbidsChange` refusal `EditSession::add_markup` raises —
    pdfcer declines rather than silently invalidating the signature. The
    `/ByteRange`/`/Reference` values are structural placeholders (no real
    signed bytes); only the census-visible structure matters to the guard.

USAGE
-----
    python tools/gen-addtext-fixtures.py

Re-run after changing anything here; the output is committed.
"""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT_DIR = ROOT / "fixtures" / "synthetic" / "addtext"

PAGE_W = 612
PAGE_H = 792

# A non-embedded Helvetica simple font (WinAnsi). The 3-key minimal std-14
# form is sufficient here (§9.6.2.2): the reader supplies the widths/outlines.
HELVETICA = (
    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
    b"/Encoding /WinAnsiEncoding >>"
)


def serialize(objects: dict[int, bytes]) -> bytes:
    """Lay `objects` out into a complete file with a classic xref table.

    §7.5.4's entry is exactly 20 bytes. No `/ID`, no timestamps -> byte-stable.
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
        f"<< /Length {len(body)} >>\nstream\n".encode("ascii")
        + body
        + b"\nendstream"
    )


def plain() -> bytes:
    """Single /Contents, page-owned /Resources."""
    content = b"BT /F1 12 Tf 72 720 Td (Original page text) Tj ET\n"
    objects: dict[int, bytes] = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: (
            f"<< /Type /Pages /Kids [3 0 R] /Count 1 "
            f"/MediaBox [0 0 {PAGE_W} {PAGE_H}] >>"
        ).encode("ascii"),
        3: (
            b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R "
            b"/Resources << /Font << /F1 5 0 R >> >> >>"
        ),
        4: stream(content),
        5: HELVETICA,
    }
    return serialize(objects)


def inherited_resources() -> bytes:
    """Two sibling pages inheriting /Resources from the /Pages node."""
    c1 = b"BT /F1 12 Tf 72 720 Td (Page one inherited) Tj ET\n"
    c2 = b"BT /F1 12 Tf 72 720 Td (Page two inherited) Tj ET\n"
    objects: dict[int, bytes] = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: (
            f"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 "
            f"/MediaBox [0 0 {PAGE_W} {PAGE_H}] "
            f"/Resources << /Font << /F1 5 0 R >> >> >>"
        ).encode("ascii"),
        # Neither page has its own /Resources -> both inherit object 2's.
        3: b"<< /Type /Page /Parent 2 0 R /Contents 6 0 R >>",
        4: b"<< /Type /Page /Parent 2 0 R /Contents 7 0 R >>",
        5: HELVETICA,
        6: stream(c1),
        7: stream(c2),
    }
    return serialize(objects)


def tagged() -> bytes:
    """A tagged page: MCID-wrapped run + /MarkInfo + minimal /StructTreeRoot."""
    content = (
        b"/P << /MCID 0 >> BDC\n"
        b"BT /F1 12 Tf 72 720 Td (Tagged run) Tj ET\n"
        b"EMC\n"
    )
    objects: dict[int, bytes] = {
        1: (
            b"<< /Type /Catalog /Pages 2 0 R "
            b"/MarkInfo << /Marked true >> /StructTreeRoot 6 0 R >>"
        ),
        2: (
            f"<< /Type /Pages /Kids [3 0 R] /Count 1 "
            f"/MediaBox [0 0 {PAGE_W} {PAGE_H}] "
            f"/Resources << /Font << /F1 5 0 R >> >> >>"
        ).encode("ascii"),
        3: (
            b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /StructParents 0 >>"
        ),
        4: stream(content),
        5: HELVETICA,
        # Minimal structure tree: one /P element wrapping MCID 0 on page 3.
        6: b"<< /Type /StructTreeRoot /K 7 0 R >>",
        7: (
            b"<< /Type /StructElem /S /P /P 6 0 R /Pg 3 0 R /K 0 >>"
        ),
    }
    return serialize(objects)


def certified_locked() -> bytes:
    """`plain.pdf` PLUS an enforced-DocMDP certification signature.

    The catalog gains `/Perms << /DocMDP 6 0 R >>` and object 6 is a signature
    dictionary carrying a `/DocMDP` transform with `/P 1` (§12.8.4 Table 258 /
    Table 254 P=1 — "no changes permitted"). `census()` reports
    `perms_enforced = true`, `signatures = 1`, `certification_permission =
    Some(1)`, so `forbids_structural_change()` is true and adding page content
    must be refused. The `/ByteRange`/`/Reference` are structural placeholders
    (no real signed bytes) — the guard only reads census-visible structure.
    """
    content = b"BT /F1 12 Tf 72 720 Td (Certified page text) Tj ET\n"
    objects: dict[int, bytes] = {
        1: (
            b"<< /Type /Catalog /Pages 2 0 R /Perms << /DocMDP 6 0 R >> >>"
        ),
        2: (
            f"<< /Type /Pages /Kids [3 0 R] /Count 1 "
            f"/MediaBox [0 0 {PAGE_W} {PAGE_H}] >>"
        ).encode("ascii"),
        3: (
            b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R "
            b"/Resources << /Font << /F1 5 0 R >> >> >>"
        ),
        4: stream(content),
        5: HELVETICA,
        # The certification signature: /Type /Sig + /ByteRange make it a
        # signature dictionary (Table 252); the /DocMDP /Reference with /P 1
        # makes it a certification with the most restrictive permission.
        6: (
            b"<< /Type /Sig /Filter /Adobe.PPKLite /ByteRange [0 1 2 3] "
            b"/Reference [ << /TransformMethod /DocMDP "
            b"/TransformParams << /P 1 >> >> ] >>"
        ),
    }
    return serialize(objects)


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    files = {
        "plain.pdf": plain(),
        "inherited-resources.pdf": inherited_resources(),
        "tagged.pdf": tagged(),
        "certified-locked.pdf": certified_locked(),
    }
    for name, data in files.items():
        (OUT_DIR / name).write_bytes(data)
        print(f"wrote {OUT_DIR / name} ({len(data)} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
