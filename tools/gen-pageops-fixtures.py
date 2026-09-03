#!/usr/bin/env python3
"""Generate the synthetic PDF fixtures the Pass 3.2 page operations use.

WHY THIS EXISTS
---------------
`docs/LEGAL.md` §5 permits only synthetic or rights-cleared PDFs in
`fixtures/`. Every file this script writes is constructed byte by byte
here, from nothing, so its provenance is this file and nothing else.

It follows the same pattern as the existing `tools/gen-*-fixtures.py`
generators: a deliberately minimal writer with no PDF library behind it,
so the fixtures cannot inherit a bug (or a normalization) from the same
code they are meant to test. In particular it emits a **classic**
cross-reference table with exactly-20-byte entries (ISO 32000-1 §7.5.4),
which is what makes these usable as delete/free-list fixtures.

WHAT IT WRITES
--------------
``fixtures/synthetic/pageops/``

``four-pages.pdf``
    Four pages, each carrying visible Helvetica text naming itself, plus:

    * an **outline** with one top-level bookmark per page — so split
      ``--bookmarks`` has something to split on and extract has bookmarks
      to subset;
    * a **link annotation** on page 1 whose destination is page 4 — so
      delete has a reference to orphan and extract has one to break;
    * a **/PageLabels** number tree — so the stale-labels disclosure is
      reachable.

    Every page inherits ``/MediaBox`` and ``/Resources`` from the root
    node rather than carrying its own. That is deliberate and is the
    single most useful property of this fixture: an extract that forgets
    to materialize inherited attributes (§7.7.3.4) produces pages with no
    ``MediaBox``, which fails loudly here and would pass on a fixture
    where every page carried its own.

``two-pages.pdf``
    Two pages, same construction, different text — the second document
    that merge and insert need.

DETERMINISM
-----------
No timestamps, no random values, no ``/ID``. Running this twice produces
byte-identical files, which is what lets the fixtures be compared rather
than merely loaded.

USAGE
-----
    python tools/gen-pageops-fixtures.py

Re-run it after changing anything here; the outputs are committed.
"""

from __future__ import annotations

import sys
from pathlib import Path

# Where the fixtures land, relative to the repository root.
OUT_DIR = Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "pageops"

# US Letter, in PDF user-space units (1/72 inch).
PAGE_WIDTH = 612
PAGE_HEIGHT = 792


def content_stream(title: str, subtitle: str) -> bytes:
    """The page's content stream: two lines of Helvetica text.

    Kept to `BT`/`Tf`/`Td`/`Tj`/`ET` plus a rectangle stroke — every
    operator here is one `pdfcer-render` implements, so a rendered
    fixture is a real visual check rather than a blank page.
    """
    body = (
        f"q\n"
        f"0.85 0.85 0.9 rg\n"
        f"36 {PAGE_HEIGHT - 120} {PAGE_WIDTH - 72} 72 re f\n"
        f"Q\n"
        f"BT\n"
        f"/F1 36 Tf\n"
        f"54 {PAGE_HEIGHT - 96} Td\n"
        f"({title}) Tj\n"
        f"ET\n"
        f"BT\n"
        f"/F1 14 Tf\n"
        f"54 {PAGE_HEIGHT - 150} Td\n"
        f"({subtitle}) Tj\n"
        f"ET\n"
    )
    return body.encode("ascii")


def build(pages: list[tuple[str, str]], *, with_extras: bool) -> bytes:
    """Assemble a complete classic-xref PDF from `pages`.

    `pages` is a list of `(title, subtitle)` pairs, one per page.
    `with_extras` adds the outline, the link annotation and the page-label
    tree — the things that make a fixture interesting to a structural
    operation rather than merely valid.

    Object numbering, fixed so the tests can name objects directly:

        1              catalog
        2              root Pages node
        3              the Helvetica font
        4 .. 3+n       page objects
        4+n .. 3+2n    content streams
        (extras)       outline root, one item per page, the link annot,
                       the page-label tree
    """
    n = len(pages)
    first_page = 4
    first_stream = first_page + n
    outline_root = first_stream + n
    first_item = outline_root + 1
    link_annot = first_item + n
    labels = link_annot + 1

    objects: dict[int, bytes] = {}

    # -- catalog ---------------------------------------------------------
    catalog = "<< /Type /Catalog /Pages 2 0 R"
    if with_extras:
        catalog += f" /Outlines {outline_root} 0 R /PageLabels {labels} 0 R"
    catalog += " >>"
    objects[1] = catalog.encode("ascii")

    # -- root Pages node --------------------------------------------------
    # MediaBox and Resources live HERE, not on the pages: see the module
    # docstring for why that is the point of this fixture.
    kids = " ".join(f"{first_page + i} 0 R" for i in range(n))
    objects[2] = (
        f"<< /Type /Pages /Kids [{kids}] /Count {n} "
        f"/MediaBox [0 0 {PAGE_WIDTH} {PAGE_HEIGHT}] "
        f"/Resources << /Font << /F1 3 0 R >> >> >>"
    ).encode("ascii")

    # -- the font ---------------------------------------------------------
    # A standard-14 face (§9.6.2.2), so no font program is embedded and
    # the fixture stays small and legally clean.
    objects[3] = (
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
        b"/Encoding /WinAnsiEncoding >>"
    )

    # -- pages and their content -------------------------------------------
    for i, (title, subtitle) in enumerate(pages):
        page = f"<< /Type /Page /Parent 2 0 R /Contents {first_stream + i} 0 R"
        if with_extras and i == 0:
            page += f" /Annots [{link_annot} 0 R]"
        page += " >>"
        objects[first_page + i] = page.encode("ascii")

        data = content_stream(title, subtitle)
        objects[first_stream + i] = (
            f"<< /Length {len(data)} >>\nstream\n".encode("ascii")
            + data
            + b"endstream"
        )

    if with_extras:
        # -- outline: one top-level bookmark per page ---------------------
        # Flat and top-level on purpose: `split --bookmarks` breaks only
        # at depth-1 entries, so a flat outline makes the expected part
        # boundaries obvious in the test.
        objects[outline_root] = (
            f"<< /Type /Outlines /First {first_item} 0 R "
            f"/Last {first_item + n - 1} 0 R /Count {n} >>"
        ).encode("ascii")
        for i, (title, _) in enumerate(pages):
            item = (
                f"<< /Title ({title}) /Parent {outline_root} 0 R "
                f"/Dest [{first_page + i} 0 R /Fit]"
            )
            if i > 0:
                item += f" /Prev {first_item + i - 1} 0 R"
            if i + 1 < n:
                item += f" /Next {first_item + i + 1} 0 R"
            item += " >>"
            objects[first_item + i] = item.encode("ascii")

        # -- a link from page 1 to the LAST page ---------------------------
        # The last page so that deleting it, or extracting without it,
        # breaks the link — which is the disclosure under test.
        objects[link_annot] = (
            f"<< /Type /Annot /Subtype /Link /Rect [54 {PAGE_HEIGHT - 170} 300 {PAGE_HEIGHT - 150}] "
            f"/Border [0 0 0] /Dest [{first_page + n - 1} 0 R /Fit] >>"
        ).encode("ascii")

        # -- page labels (§12.4.2) ------------------------------------------
        objects[labels] = b"<< /Nums [0 << /S /D /St 1 >>] >>"

    return serialize(objects)


def serialize(objects: dict[int, bytes]) -> bytes:
    """Lay out `objects` into a complete file with a classic xref table.

    §7.5.4's entry format is exactly 20 bytes: ten digits, a space, five
    digits, a space, the keyword, then a two-byte EOL. `%05d`/`%010d`
    plus a trailing `" \\n"` is that, and it is written out longhand here
    rather than through a helper so the byte count is visible.
    """
    out = bytearray(b"%PDF-1.7\n")
    # §7.5.2: a comment line with four bytes >= 128 marks the file binary.
    out += b"%\xe2\xe3\xcf\xd3\n"

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


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)

    four = build(
        [
            ("Page One", "Chapter 1 - the first page of the fixture"),
            ("Page Two", "Chapter 2 - the second page"),
            ("Page Three", "Chapter 3 - the third page"),
            ("Page Four", "Chapter 4 - the last page, linked from page 1"),
        ],
        with_extras=True,
    )
    two = build(
        [
            ("Insert A", "First page of the second fixture document"),
            ("Insert B", "Second page of the second fixture document"),
        ],
        with_extras=False,
    )

    for name, data in (("four-pages.pdf", four), ("two-pages.pdf", two)):
        path = OUT_DIR / name
        path.write_bytes(data)
        print(f"wrote {path} - {len(data)} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
