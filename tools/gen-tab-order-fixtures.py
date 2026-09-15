#!/usr/bin/env python3
"""Generate the synthetic ``/Tabs`` tab-order fixtures.

WHY THIS EXISTS
---------------
``crates/pdfcer-core/src/edit.rs``'s ``EditSession::page_tab_sequence``
derives the order a reader visits a page's annotations in — ISO 32000-1
§12.5.1's ``/R``/``/C``/``/S`` bullets, ISO 32000-2's ``/A`` and ``/W``,
the absent case, and a value outside the standard's closed set. The
``pdfcer tab-order`` command is its shell.

Core unit tests pin the derivation. They structurally cannot pin the
shell: the ``--pages`` and ``--row-tolerance`` flags reaching the call,
the 0-based/1-based page boundary, the six ``basis=`` tokens, the
``skipped=`` reasons, and — the one that matters most here — that the
rule-4 ``note`` lines are printed **on stdout**, where a script that
keeps stdout and discards stderr still receives them.

``docs/LEGAL.md`` §5 permits only synthetic or rights-cleared PDFs under
``fixtures/``. Every byte here is constructed from nothing, with **no PDF
library behind it**, following ``tools/gen-link-fixtures.py``'s pattern
and making its argument: a fixture produced by a library inherits that
library's normalisations, and several of the cases below are things no
authoring tool will emit on request — a ``/Tabs`` value outside the
closed set, an annotation dictionary written directly into ``/Annots``,
a ``NoView`` annotation that also sets ``ToggleNoView``.

WHY THE ARRAY ORDER IS NEVER THE ANSWER
---------------------------------------
★ On every page below, ``/Annots`` is listed in an order that is **none**
of the orders any test expects. That is the whole design. A verb that
quietly returned the array would pass against a fixture listed in row
order, and the project has a standing lesson for exactly that shape: a
fixture whose default value equals the expected value cannot falsify
anything.

WHAT IT WRITES
--------------
``fixtures/synthetic/tab-order/``

``modes.pdf``
    Seven pages, one per ``/Tabs`` state, each carrying the SAME four
    annotations in the same scrambled array — so the only thing that
    differs between pages is the value being tested.

    ===== ============ =====================================================
    page  ``/Tabs``    what it pins
    ===== ============ =====================================================
    1     ``/A``       the file states the order; no notes at all
    2     ``/W``       widgets first, then the contested tail (``TAB-A1``)
    3     ``/R``       row order, computed from geometry
    4     ``/C``       column order — a DIFFERENT answer on the same grid
    5     ``/S``       not derived: no ``tab`` lines, and that is correct
    6     *(absent)*   array order used as a convention, disclosed as one
    7     ``/Q``       outside the closed set; reported verbatim
    ===== ============ =====================================================

    The grid is deliberately square-ish, so row order and column order
    disagree. Pages 3 and 4 differing is the single most informative
    assertion in the whole suite.

``geometry.pdf``
    Three pages for the parts of the derivation that are geometric or
    structural rather than declarative:

    * **page 1** — ``/Tabs /R`` with ``/Rotate 90``. §12.5.1 says its
      descriptions *"assume the page is being viewed in the orientation
      specified by the Rotate entry"* while §12.5.3 says ``/Rect``
      *"continues to describe the annotation's relationship with the
      unscaled, unrotated user space"*. A reader that groups on raw
      ``/Rect`` y is silently wrong here and nowhere else.
    * **page 2** — the five annotations a reader does not tab to, plus
      one it does: ``Hidden`` (``/F 2``), ``NoView`` (``/F 32``),
      ``NoView + ToggleNoView`` (``/F 288``, which **stays**), a
      ``/Popup``, and a ``/TrapNet``. Also **one annotation dictionary
      written directly into ``/Annots``** — legal per Table 164, with no
      identity to be named by, so it can only ever be disclosed.
    * **page 3** — two widgets whose tops are **2 points** apart, left
      and right swapped. One row at ``--row-tolerance 3``, two rows at
      the shipped 1. The page exists so the flag has something to move.

``rtl.pdf``
    One page, ``/Tabs /R``, and the catalog carries
    ``/ViewerPreferences << /Direction /R2L >>``. §12.5.1 makes the
    direction within a row a ``shall`` determined by that entry, so this
    is a conformance case and not a preference. A separate file because
    ``/Direction`` is document-level: it cannot be varied per page, and
    a fixture that tried would be testing nothing.
"""

from pathlib import Path

OUT_DIR = Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "tab-order"

PAGE_W = 612
PAGE_H = 792

# The 2x2 grid every `modes.pdf` page carries. Named by where they sit on
# an unrotated page, because that is what the expectations are written in.
TOP_LEFT = "[100 700 200 720]"
TOP_RIGHT = "[400 700 500 720]"
BOTTOM_LEFT = "[100 600 200 620]"
BOTTOM_RIGHT = "[400 600 500 620]"


def serialize(objects: dict[int, bytes], catalog_extra: str = "") -> bytes:
    """Lay out `objects` into a complete classic-xref file (§7.5.4).

    Entry format is exactly 20 bytes: ten digits, a space, five digits, a
    space, the keyword, a two-byte EOL. Object numbers with no body are
    emitted as free entries.
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


def widget(rect: str, name: str) -> bytes:
    """A text-field widget. ``/Ff 0`` so nothing here is read-only."""
    return (
        f"<< /Type /Annot /Subtype /Widget /FT /Tx /T ({name}) /Rect {rect} "
        f"/F 4 /Border [0 0 0] >>"
    ).encode("ascii")


def annot(subtype: str, rect: str, extra: str = "") -> bytes:
    return (
        f"<< /Type /Annot /Subtype /{subtype} /Rect {rect} /Border [0 0 0] {extra} >>"
    ).encode("ascii")


def write(name: str, data: bytes) -> None:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    (OUT_DIR / name).write_bytes(data)
    print(f"  {name}  {len(data)} bytes")


def gen_modes() -> None:
    """Seven pages, one `/Tabs` state each, one shared annotation set.

    The four annotations are shared objects referenced from every page's
    `/Annots`. That is unusual — an annotation "shall" belong to one page
    — but the array here is what is under test and every page lists the
    same four in the same scrambled order, which is exactly the property
    that makes page 3's answer and page 4's answer comparable. A reader
    of this file should not copy that structure into a real document.
    """
    tabs = ["/Tabs /A", "/Tabs /W", "/Tabs /R", "/Tabs /C", "/Tabs /S", "", "/Tabs /Q"]
    count = len(tabs)
    first_annot = 3 + count
    kids = " ".join(f"{3 + i} 0 R" for i in range(count))
    objects: dict[int, bytes] = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: (
            f"<< /Type /Pages /Kids [{kids}] /Count {count} "
            f"/MediaBox [0 0 {PAGE_W} {PAGE_H}] /Resources << >> >>"
        ).encode("ascii"),
    }
    tl, tr, bl, br = (first_annot + i for i in range(4))
    # Two widgets and two non-widgets, so /Tabs /W has something to
    # separate. The widgets are the TOP row, so /W's first pass and row
    # order agree on the first two entries and disagree after -- which
    # makes a mix-up visible rather than accidentally correct.
    objects[tl] = widget(TOP_LEFT, "tl")
    objects[tr] = widget(TOP_RIGHT, "tr")
    objects[bl] = annot("Link", BOTTOM_LEFT)
    objects[br] = annot("Text", BOTTOM_RIGHT, "/Contents (br)")
    # NONE of row, column, widget or reverse -- see the module docstring.
    scrambled = f"[{br} 0 R {tr} 0 R {bl} 0 R {tl} 0 R]"
    for i, tab in enumerate(tabs):
        objects[3 + i] = (
            f"<< /Type /Page /Parent 2 0 R /Annots {scrambled} {tab} >>"
        ).encode("ascii")
    write("modes.pdf", serialize(objects))


def gen_geometry() -> None:
    """Three pages: rotation, the skipped kinds, and a tolerance edge."""
    objects: dict[int, bytes] = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: (
            f"<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 "
            f"/MediaBox [0 0 {PAGE_W} {PAGE_H}] /Resources << >> >>"
        ).encode("ascii"),
    }

    # ---- page 1: /Rotate 90. Rotating the page clockwise turns the LEFT
    # column into the TOP row, so the visit order is bl, tl, br, tr --
    # which is neither the array order nor the unrotated row order.
    objects[6] = widget(TOP_LEFT, "tl")
    objects[7] = widget(TOP_RIGHT, "tr")
    objects[8] = widget(BOTTOM_LEFT, "bl")
    objects[9] = widget(BOTTOM_RIGHT, "br")
    objects[3] = (
        b"<< /Type /Page /Parent 2 0 R /Annots [9 0 R 7 0 R 8 0 R 6 0 R] "
        b"/Tabs /R /Rotate 90 >>"
    )

    # ---- page 2: everything a reader does not tab to, plus a direct
    # dictionary entry that has no identity to be named by.
    objects[10] = (
        b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (hidden) "
        b"/Rect [100 700 200 720] /F 2 /Border [0 0 0] >>"
    )
    objects[11] = (
        b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (noview) "
        b"/Rect [200 700 300 720] /F 32 /Border [0 0 0] >>"
    )
    # /F 288 = NoView (32) + ToggleNoView (256). ISO 32000-2 says
    # ToggleNoView inverts NoView for "annotation selection", and tabbing
    # selects -- so this one STAYS in the sequence.
    objects[12] = (
        b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (toggle) "
        b"/Rect [300 700 400 720] /F 288 /Border [0 0 0] >>"
    )
    objects[13] = annot("Popup", "[400 700 500 720]")
    # §14.11.6.2: a conforming trap network has Print and ReadOnly set
    # and every other flag clear. 4 | 64 = 68.
    objects[14] = annot("TrapNet", "[500 700 600 720]", "/F 68")
    objects[15] = widget("[100 600 200 620]", "visible")
    objects[4] = (
        b"<< /Type /Page /Parent 2 0 R /Tabs /R /Annots "
        b"[10 0 R 11 0 R 12 0 R 13 0 R "
        b"<< /Type /Annot /Subtype /Square /Rect [100 500 200 520] >> "
        b"14 0 R 15 0 R] >>"
    )

    # ---- page 3: tops two points apart, sides swapped. One row or two,
    # depending entirely on --row-tolerance.
    objects[16] = widget("[400 700 500 720]", "right-higher")
    objects[17] = widget("[100 698 200 718]", "left-lower")
    objects[5] = b"<< /Type /Page /Parent 2 0 R /Tabs /R /Annots [16 0 R 17 0 R] >>"

    write("geometry.pdf", serialize(objects))


def gen_rtl() -> None:
    """One page, row order, and a document that reads right to left."""
    objects: dict[int, bytes] = {
        1: (
            b"<< /Type /Catalog /Pages 2 0 R "
            b"/ViewerPreferences << /Direction /R2L >> >>"
        ),
        2: (
            f"<< /Type /Pages /Kids [3 0 R] /Count 1 "
            f"/MediaBox [0 0 {PAGE_W} {PAGE_H}] /Resources << >> >>"
        ).encode("ascii"),
        4: widget(TOP_LEFT, "tl"),
        5: widget(TOP_RIGHT, "tr"),
        6: widget(BOTTOM_LEFT, "bl"),
        7: widget(BOTTOM_RIGHT, "br"),
        3: b"<< /Type /Page /Parent 2 0 R /Annots [7 0 R 5 0 R 6 0 R 4 0 R] /Tabs /R >>",
    }
    write("rtl.pdf", serialize(objects))


def main() -> None:
    print(f"writing {OUT_DIR}")
    gen_modes()
    gen_geometry()
    gen_rtl()


if __name__ == "__main__":
    main()
