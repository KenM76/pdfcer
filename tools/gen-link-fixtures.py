#!/usr/bin/env python3
"""Generate the synthetic ``/Link`` annotation destination fixtures.

WHY THIS EXISTS
---------------
``crates/pdfcer-core/src/annot.rs``'s ``page_link_destinations`` and
``crates/pdfcer-core/src/outline.rs``'s ``DestinationReader`` resolve
ISO 32000-1 §12.5.6.5 (Table 173) link annotations to §12.3.2
destinations. That reader has to be right about a set of cases that no
authoring tool will produce on request:

* a link whose ``/Dest`` and ``/A`` are **both** present, which Table 173
  forbids (*"shall not be present if an A entry is present"*) and which
  real sanitisers nonetheless leave behind;
* a link naming an object that is **not a page in the page tree** — the
  ordinary residue of a page delete, and the case that must never be
  reported as page 1;
* a link naming a destination **name** that neither §12.3.2.3 namespace
  defines, which is what a page-range extraction leaves when it drops
  ``/Names``;
* a link whose action is not a navigation at all (``/URI``,
  ``/JavaScript``), which must be disclosed and never followed;
* a ``/GoToR`` into another file, whose name must **not** be resolved
  against this document's namespaces even when this document happens to
  define it.

``docs/LEGAL.md`` §5 permits only synthetic or rights-cleared PDFs under
``fixtures/``. Every byte here is constructed from nothing, with **no PDF
library behind it**, following the existing ``tools/gen-*-fixtures.py``
pattern — the same argument ``gen-outline-fixtures.py`` makes: a fixture
produced by a library inherits that library's normalisations and can
therefore no longer express most of the malformations above. Classic
cross-reference table (§7.5.4) throughout.

WHY THE FIXTURES ARE MULTI-PAGE
-------------------------------
★ Every fixture here has **at least three pages, and no link points at
page 1.** That is not padding. A destination resolver's single most
likely defect is returning a defaulted ``0`` page index, and a one-page
fixture — or a multi-page fixture whose links all target the first page —
passes against an implementation that resolved nothing at all. The
project has a standing lesson for exactly this shape: a default-valued
fixture cannot falsify a carry.

WHAT IT WRITES
--------------
``fixtures/synthetic/links/``

``goto-actions.pdf``
    Four pages. Four links on page 1, each a ``/GoTo`` action with an
    explicit destination array, covering the four view styles a viewer
    must get right: ``/Fit``, ``/XYZ`` **with a null zoom** (Table 151's
    *retain current magnification*, which a reader that coerces null to
    ``0`` turns into an infinite zoom-out), ``/FitH``, and ``/FitR``.
    Targets are pages 2, 3, 4 and 2 — never page 1.

``named-links.pdf``
    Three pages. Three links resolved **by name**, covering both
    §12.3.2.3 namespaces and their failure mode:

    * ``/A << /S /GoTo /D (tree-target) >>`` — a byte string into the
      PDF 1.2 ``/Names → /Dests`` name tree.
    * ``/Dest /LegacyTarget`` — a name object into the PDF 1.1 catalog
      ``/Dests`` dictionary, written as a direct ``/Dest`` rather than an
      action, which is the older spelling and a separate code path.
    * ``/A << /S /GoTo /D (absent-target) >>`` — defined by **neither**
      namespace, which must survive as a reported unresolved name rather
      than vanish.

``broken-links.pdf``
    Three pages. The malformed set, all on page 1:

    * a link naming an object that exists but is **not a page**;
    * a link naming ``99 0 R``, a genuinely **dangling** reference (a free
      xref entry, so §7.3.10 makes it null);
    * a link with **neither** ``/Dest`` nor ``/A`` — visible, clickable,
      and able to do nothing, which must be *counted* and not silently
      dropped;
    * a link with **both** ``/Dest`` and ``/A``, pointing at *different*
      pages (3 and 2 respectively) so that which one wins is observable
      rather than a coin flip. ``/Dest`` wins, matching the outline path.

``non-navigation-links.pdf``
    Three pages. Links whose actions are not page jumps — ``/URI``,
    ``/JavaScript``, ``/Launch``, and a ``/GoToR`` into ``other.pdf``
    whose ``/D`` is the byte string ``(tree-target)``.

    ★ **This document deliberately DEFINES ``tree-target`` in its own
    name tree, pointing at page 3.** A resolver that resolved a
    ``/GoToR``'s name against the local namespace would report a
    confident, entirely wrong local page jump — the failure mode
    ``outline.rs``'s ``read_remote`` exists to prevent, and one that a
    fixture without the colliding name cannot detect.

``no-links.pdf``
    Two pages, one ``/Square`` annotation, no links at all. The
    zero-result control: a tool that reports nothing here must still be
    distinguishable from a tool that failed to look.
"""

from pathlib import Path

OUT_DIR = Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "links"

PAGE_W = 612
PAGE_H = 792


def serialize(objects: dict[int, bytes]) -> bytes:
    """Lay out `objects` into a complete classic-xref file (§7.5.4).

    Entry format is exactly 20 bytes: ten digits, a space, five digits, a
    space, the keyword, a two-byte EOL. Object numbers with no body are
    emitted as **free** entries, which is what lets ``broken-links.pdf``
    reference ``99 0 R`` and have it genuinely dangle (§7.3.10) rather
    than be a parse error.
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


def pages_doc(count: int, catalog_extra: str = "") -> tuple[dict[int, bytes], int]:
    """A `count`-page skeleton with no annotations yet.

    Object 1 is the catalog, object 2 the page-tree root, objects
    ``3..3+count-1`` the pages. Returns the object table and the first
    free object number, so a fixture can lay annotations out above the
    pages without hard-coding arithmetic that breaks when a page is
    added.
    """
    kids = " ".join(f"{3 + i} 0 R" for i in range(count))
    objects: dict[int, bytes] = {
        1: f"<< /Type /Catalog /Pages 2 0 R {catalog_extra} >>".encode("ascii"),
        2: (
            f"<< /Type /Pages /Kids [{kids}] /Count {count} "
            f"/MediaBox [0 0 {PAGE_W} {PAGE_H}] /Resources << >> >>"
        ).encode("ascii"),
    }
    for i in range(count):
        objects[3 + i] = b"<< /Type /Page /Parent 2 0 R >>"
    return objects, 3 + count


def attach_annots(objects: dict[int, bytes], page_num: int, annot_nums: list[int]) -> None:
    """Give page object `page_num` an ``/Annots`` array.

    §7.7.3.4: ``/Annots`` is **not** inheritable, so it is written on the
    page itself and never on the ``/Pages`` node — a fixture that put it
    on the parent would silently test nothing.
    """
    refs = " ".join(f"{n} 0 R" for n in annot_nums)
    objects[page_num] = (
        f"<< /Type /Page /Parent 2 0 R /Annots [{refs}] >>".encode("ascii")
    )


def link(rect: str, body: str) -> bytes:
    """A ``/Link`` annotation (Table 173) with the given `/Rect` and body.

    ``/Border [0 0 0]`` on every one of them: §12.5.6.5's default border
    is a **visible** one-unit-wide box, and a fixture that drew borders
    would make every render-comparison test that ever opens these files
    depend on the border-drawing code as well as the link-reading code.
    """
    return f"<< /Type /Annot /Subtype /Link /Rect [{rect}] /Border [0 0 0] {body} >>".encode(
        "ascii"
    )


# ---------------------------------------------------------------------
# goto-actions.pdf
# ---------------------------------------------------------------------
def goto_actions() -> bytes:
    """Four `/GoTo` links covering the four view styles that matter.

    ``/XYZ 72 720 null`` is the load-bearing one. Table 151 defines a
    null zoom as *retain the current magnification*, and it is
    syntactically an ordinary array element — so a reader that reaches
    for ``as_number()`` and falls back to zero produces a destination
    that zooms the page to nothing, on a file that is entirely valid.
    """
    objects, free = pages_doc(4)
    a1, a2, a3, a4 = free, free + 1, free + 2, free + 3
    objects[a1] = link("36 700 200 730", "/A << /S /GoTo /D [4 0 R /Fit] >>")
    objects[a2] = link("36 660 200 690", "/A << /S /GoTo /D [5 0 R /XYZ 72 720 null] >>")
    objects[a3] = link("36 620 200 650", "/A << /S /GoTo /D [6 0 R /FitH 540] >>")
    objects[a4] = link("36 580 200 610", "/A << /S /GoTo /D [4 0 R /FitR 76 119 536 673] >>")
    attach_annots(objects, 3, [a1, a2, a3, a4])
    return serialize(objects)


# ---------------------------------------------------------------------
# named-links.pdf
# ---------------------------------------------------------------------
def named_links() -> bytes:
    """Both §12.3.2.3 namespaces, plus a name that neither defines."""
    objects, free = pages_doc(3, catalog_extra="/Names 20 0 R /Dests 21 0 R")
    a1, a2, a3 = free, free + 1, free + 2
    objects[a1] = link("36 700 200 730", "/A << /S /GoTo /D (tree-target) >>")
    # The PDF 1.1 spelling: a NAME object, written straight into /Dest
    # with no action wrapper at all.
    objects[a2] = link("36 660 200 690", "/Dest /LegacyTarget")
    objects[a3] = link("36 620 200 650", "/A << /S /GoTo /D (absent-target) >>")
    attach_annots(objects, 3, [a1, a2, a3])
    # §7.9.6 name tree, single leaf node. Keys are STRINGS and must be
    # sorted; one key needs no ordering argument but the /Limits are
    # written anyway because a reader that requires them on a root leaf
    # is wrong and this fixture should not be what proves it.
    objects[20] = b"<< /Dests 22 0 R >>"
    objects[21] = b"<< /LegacyTarget [5 0 R /Fit] >>"
    objects[22] = (
        b"<< /Limits [(tree-target) (tree-target)] "
        b"/Names [(tree-target) [4 0 R /XYZ null 792 null]] >>"
    )
    return serialize(objects)


# ---------------------------------------------------------------------
# broken-links.pdf
# ---------------------------------------------------------------------
def broken_links() -> bytes:
    """The four malformations a link reader must survive and report.

    One object exists and is deliberately **not** a page: a reader that
    checks only "did this resolve to a dictionary" accepts it, and only a
    reader that checks membership in the page tree rejects it. Object 99
    is never written, so its xref entry is free and §7.3.10 makes the
    reference null — a different failure, and the two must not be
    collapsed.
    """
    objects, free = pages_doc(3)
    a1, a2, a3, a4 = free, free + 1, free + 2, free + 3
    # The not-a-page object is numbered ABOVE the annotations, not at a
    # hand-picked low number. Writing it as `9` collided with `a4` (the
    # fourth annotation, also 9 here) and silently deleted that link —
    # the fixture then "passed" while testing three cases instead of
    # four. Found by running the CLI against it, not by any test.
    not_a_page = free + 4
    objects[a1] = link("36 700 200 730", f"/A << /S /GoTo /D [{not_a_page} 0 R /Fit] >>")
    objects[a2] = link("36 660 200 690", "/A << /S /GoTo /D [99 0 R /Fit] >>")
    objects[a3] = link("36 620 200 650", "")
    # Both present, pointing at DIFFERENT pages, so precedence is
    # observable. /Dest is page 3 (object 5); /A is page 2 (object 4).
    objects[a4] = link(
        "36 580 200 610",
        "/Dest [5 0 R /Fit] /A << /S /GoTo /D [4 0 R /Fit] >>",
    )
    attach_annots(objects, 3, [a1, a2, a3, a4])
    objects[not_a_page] = b"<< /Type /Metadata /Subtype /XML >>"
    return serialize(objects)


# ---------------------------------------------------------------------
# non-navigation-links.pdf
# ---------------------------------------------------------------------
def non_navigation_links() -> bytes:
    """Actions that are not page jumps, plus the `/GoToR` name trap.

    The name tree defines ``tree-target`` → page 3, and the ``/GoToR``
    link's ``/D`` is that same byte string. §12.6.4.3 puts a remote
    destination's name in the **target** file's namespace, so resolving
    it here would produce a wrong answer that looks completely correct.
    That is the whole reason the collision is written in.
    """
    objects, free = pages_doc(3, catalog_extra="/Names 20 0 R")
    a1, a2, a3, a4 = free, free + 1, free + 2, free + 3
    objects[a1] = link(
        "36 700 200 730", "/A << /S /URI /URI (https://example.invalid/spec) >>"
    )
    objects[a2] = link(
        "36 660 200 690", "/A << /S /JavaScript /JS (app.alert\\(1\\);) >>"
    )
    objects[a3] = link("36 620 200 650", "/A << /S /Launch /F (payload.exe) >>")
    objects[a4] = link(
        "36 580 200 610",
        "/A << /S /GoToR /F (other.pdf) /D (tree-target) /NewWindow true >>",
    )
    attach_annots(objects, 3, [a1, a2, a3, a4])
    objects[20] = b"<< /Dests 21 0 R >>"
    objects[21] = (
        b"<< /Limits [(tree-target) (tree-target)] "
        b"/Names [(tree-target) [5 0 R /Fit]] >>"
    )
    return serialize(objects)


# ---------------------------------------------------------------------
# no-links.pdf
# ---------------------------------------------------------------------
def no_links() -> bytes:
    """The zero-result control: annotations, but no `/Link` among them."""
    objects, free = pages_doc(2)
    objects[free] = (
        b"<< /Type /Annot /Subtype /Square /Rect [36 700 200 730] "
        b"/IC [1 0 0] >>"
    )
    attach_annots(objects, 3, [free])
    return serialize(objects)


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    files = {
        "goto-actions.pdf": goto_actions(),
        "named-links.pdf": named_links(),
        "broken-links.pdf": broken_links(),
        "non-navigation-links.pdf": non_navigation_links(),
        "no-links.pdf": no_links(),
    }
    for name, data in files.items():
        (OUT_DIR / name).write_bytes(data)
        print(f"wrote {name} ({len(data)} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
