#!/usr/bin/env python3
"""Generate the synthetic document-outline (bookmark) fixtures.

WHY THIS EXISTS
---------------
`crates/pdfcer-core/src/outline.rs` reads ISO 32000-1 §12.3.3's outline
hierarchy and §12.3.2's destinations. Almost every interesting case in
that reader is a case a normal authoring tool will not produce on
request: a ``/Next`` chain that loops, a ``/Count`` whose magnitude lies,
a destination naming a page object that is not in the page tree, an
outline nested forty levels deep. Those are exactly the inputs a reader
must survive, and exactly the inputs no real-world PDF hands you on
demand.

`docs/LEGAL.md` §5 permits only synthetic or rights-cleared PDFs under
``fixtures/``. Every byte this script writes is constructed here, from
nothing, with **no PDF library behind it** — following the existing
``tools/gen-*-fixtures.py`` pattern. That is not merely a licensing
convenience: a fixture produced by a PDF library inherits that library's
normalizations, and would therefore be unable to express most of the
malformations above. It emits a **classic** cross-reference table
(§7.5.4).

WHAT IT WRITES
--------------
``fixtures/synthetic/outline/``

``basic-tree.pdf``
    Five pages. A two-root outline: "Chapter 1" (open, two children) and
    "Chapter 2" (**closed** — ``/Count -1`` — one child). Exercises the
    four ``must_have`` view types from
    ``Acrobat_Features/bookmarks__destinations_and_navigation.md``:
    ``/XYZ`` (with a ``null`` zoom), ``/Fit``, ``/FitH`` and ``/FitR``.

    **Every ``/Count`` in this file is correct**, which is the point: it
    is the happy-path fixture, and a reader must report it as a faithful
    transcription with no diagnostics at all. The counts follow §12.3.3's
    two different rules — an item's ``/Count`` magnitude is its *visible
    descendants*, excluding itself, while the **root's** counts all
    visible items *including* the top-level ones. So "Chapter 1" (open,
    two leaf children) is ``+2``, "Chapter 2" (closed, one child) is
    ``-1``, and the root is ``4`` — the two chapters, plus Chapter 1's
    two children, and nothing from behind Chapter 2's closed node.

``lying-counts.pdf``
    Three pages. The same shape, with the magnitudes **deliberately
    wrong and the signs deliberately right**: "Open" declares ``/Count
    +9`` with two children, "Shut" declares ``/Count -7`` with one, and
    the root declares ``99``. A reader that treats ``/Count`` as a child
    count reports nine children; a reader that reads only the sign
    reports two, open. Separated from ``basic-tree.pdf`` so that one
    fixture pins the structural rule (sign wins, magnitude is ignored)
    and the other pins the clean case — and so the magnitude
    cross-check has something to disagree with.

``named-dests.pdf``
    Three pages. Four outline items whose destinations are **names**, not
    arrays, covering both of §12.3.2.3's namespaces and their failure
    mode:
      * ``/Dest /LegacyIntro`` — the PDF 1.1 catalog ``/Dests``
        **dictionary**, keyed by name objects.
      * ``/Dest (tree-body)`` — the PDF 1.2 ``/Names → /Dests`` **name
        tree** (§7.9.6), reached through a ``/Kids`` intermediate node so
        the tree walk is genuinely exercised rather than a single leaf.
      * ``/Dest (tree-wrapped)`` — the same tree, but the value is a
        ``<< /D [...] >>`` **dictionary** rather than a bare array.
      * ``/Dest (tree-action)`` — a name-tree value wrapping a **go-to
        action** (``<< /A << /S /GoTo /D [...] >> >>``) rather than a
        ``/D``, which §12.3.2.3 NOTE 2 explicitly permits.
      * ``/Dest (LegacyIntro)`` — the legacy dictionary's key spelled as
        a **string**, so it resolves only because pdfcer searches both
        namespaces regardless of the reference's type. Pins the
        ``DEST-A1`` disclosure.
      * ``/Dest (nowhere)`` — a name defined by neither namespace. Must
        survive as an unresolved name, not vanish.

``actions.pdf``
    Two pages. Five items reached through ``/A`` action dictionaries
    (§12.6) instead of ``/Dest``: ``/GoTo`` (same document), ``/GoToR``
    with an integer page number and with a named target in the remote
    file, ``/URI``, and ``/JavaScript``. The last two are the
    recognize-and-disclose-never-execute cases.

``both-dest-and-a.pdf``
    Two pages, one item carrying **both** ``/Dest`` (page 0) and a
    ``/GoTo`` ``/A`` (page 1). §12.3.3 makes these mutually exclusive, so
    the file is malformed by construction; the two point at *different*
    pages precisely so a test can prove which one won.

``broken-dests.pdf``
    Two pages. Three items whose destinations cannot reach a page index:
    one naming object ``99 0 R`` (which does not exist), one naming an
    object that exists but is **not a page** (the catalog), and one whose
    destination array is empty. All three must be reported, none dropped.

``cycle.pdf``
    One page. Three separate loops in one file: two siblings whose
    ``/Next`` entries point at each other, an item whose ``/First`` points
    at itself, and an item whose ``/First`` points back at the root's
    first child. A reader without a cycle guard hangs here; that is the
    whole point of the file.

``deep.pdf``
    One page. A single chain forty levels deep — past the reader's
    documented nesting cap — so the depth guard is exercised as a
    *reported truncation* rather than a stack overflow.

``titles.pdf``
    One page. Four titles pinning §7.9.2 text-string decoding at the
    outline layer: plain ASCII; UTF-16BE with the ``FE FF`` BOM and a
    non-Latin script; a PDFDocEncoding byte that is *not* Latin-1
    (``0xA0`` is EURO, not NBSP); and an **undefined** PDFDocEncoding code
    (``0xAD``) that must be disclosed as inexact rather than silently
    dropped.

``no-outline.pdf``
    One page, no ``/Outlines`` in the catalog at all. The empty-not-error
    case.
"""

from pathlib import Path

OUT_DIR = Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "outline"

PAGE_W = 612
PAGE_H = 792


def serialize(objects: dict[int, bytes]) -> bytes:
    """Lay out `objects` into a complete classic-xref file (§7.5.4).

    Entry format is exactly 20 bytes: ten digits, a space, five digits, a
    space, the keyword, a two-byte EOL — written longhand so the byte
    count is visible. Object numbers with no body are emitted as free
    entries, which is what lets a fixture reference ``99 0 R`` and have it
    genuinely dangle (§7.3.10) rather than be a parse error.
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


def pdf_string(raw: bytes) -> bytes:
    """A literal string object (§7.3.4.2) with the three escapes that matter.

    Backslash, and both parentheses, are escaped; everything else is
    emitted as-is, including bytes above 0x7F. That last part is
    deliberate — ``titles.pdf`` needs raw 0xA0 and 0xAD bytes to reach the
    decoder unchanged, and a "helpful" hex-string fallback would hide the
    very byte the test is about.
    """
    body = raw.replace(b"\\", b"\\\\").replace(b"(", b"\\(").replace(b")", b"\\)")
    return b"(" + body + b")"


def pages_doc(count: int, catalog_extra: str = "") -> tuple[dict[int, bytes], int]:
    """A `count`-page skeleton.

    Object 1 is the catalog, object 2 the page-tree root, objects 3..3+n-1
    the pages. Returns the object table and the first free object number,
    so each fixture can lay its outline objects out above the pages
    without hard-coding arithmetic that breaks when a page is added.
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
        objects[3 + i] = f"<< /Type /Page /Parent 2 0 R >>".encode("ascii")
    return objects, 3 + count


def item(
    title: bytes,
    parent: int,
    *,
    prev: int | None = None,
    nxt: int | None = None,
    first: int | None = None,
    last: int | None = None,
    count: int | None = None,
    dest: str | None = None,
    action: str | None = None,
    extra: str = "",
) -> bytes:
    """One §12.3.3 Table 153 outline **item** dictionary.

    Every structural key is optional at this layer on purpose: several
    fixtures exist precisely to present an item with a key missing or
    pointing somewhere it should not, and a builder that "fixed up" the
    links would generate the well-formed file instead of the one under
    test.
    """
    parts = [b"<< /Title ", pdf_string(title), f" /Parent {parent} 0 R".encode("ascii")]
    if prev is not None:
        parts.append(f" /Prev {prev} 0 R".encode("ascii"))
    if nxt is not None:
        parts.append(f" /Next {nxt} 0 R".encode("ascii"))
    if first is not None:
        parts.append(f" /First {first} 0 R".encode("ascii"))
    if last is not None:
        parts.append(f" /Last {last} 0 R".encode("ascii"))
    if count is not None:
        parts.append(f" /Count {count}".encode("ascii"))
    if dest is not None:
        parts.append(f" /Dest {dest}".encode("ascii"))
    if action is not None:
        parts.append(f" /A {action}".encode("ascii"))
    if extra:
        parts.append(f" {extra}".encode("ascii"))
    parts.append(b" >>")
    return b"".join(parts)


def root(first: int, last: int, count: int) -> bytes:
    """The §12.3.3 Table 152 outline **root** dictionary.

    ``/Count`` here is the number of *visible* items at all levels, and
    carries no open/closed meaning of its own — the root is always
    "open" in the sense that it has no collapsed state to store.
    """
    return (
        f"<< /Type /Outlines /First {first} 0 R /Last {last} 0 R /Count {count} >>"
    ).encode("ascii")


# ---------------------------------------------------------------------
# basic-tree.pdf
# ---------------------------------------------------------------------
def basic_tree() -> bytes:
    """Two roots, five items, the four must_have view types, counts CORRECT.

    Layout (object numbers in brackets):

        [9]  Chapter 1        /Count +2  -> OPEN,  2 children
             [11] Section 1.1  [3 0 R /XYZ 72 720 null]
             [12] Section 1.2  [4 0 R /FitH 700]
        [10] Chapter 2        /Count -1  -> CLOSED, 1 child
             [13] Section 2.1  [6 0 R /FitR 10 20 300 400]

    Root ``/Count 4``: the two chapters plus Chapter 1's two visible
    children. Chapter 2 is closed, so its child contributes nothing to
    the root total even though Chapter 2 itself does.

    Chapter 1's own destination is ``[3 0 R /Fit]``; Chapter 2's is
    ``[5 0 R /XYZ null null 2]`` — a zoom-only ``/XYZ`` whose left and top
    are ``null``, the §12.3.2.2 "retain the current value" form.
    """
    objects, n = pages_doc(5, "/Outlines 8 0 R")
    assert n == 8, n
    objects[8] = root(9, 10, 4)
    objects[9] = item(
        b"Chapter 1",
        8,
        nxt=10,
        first=11,
        last=12,
        count=2,  # positive => OPEN; 2 visible descendants
        dest="[3 0 R /Fit]",
    )
    objects[10] = item(
        b"Chapter 2",
        8,
        prev=9,
        first=13,
        last=13,
        count=-1,  # negative => CLOSED; 1 descendant if reopened
        dest="[5 0 R /XYZ null null 2]",
    )
    objects[11] = item(b"Section 1.1", 9, nxt=12, dest="[3 0 R /XYZ 72 720 null]")
    objects[12] = item(b"Section 1.2", 9, prev=11, dest="[4 0 R /FitH 700]")
    objects[13] = item(b"Section 2.1", 10, dest="[6 0 R /FitR 10 20 300 400]")
    return serialize(objects)


# ---------------------------------------------------------------------
# lying-counts.pdf
# ---------------------------------------------------------------------
def lying_counts() -> bytes:
    """Right signs, wrong magnitudes, wrong root total.

        [6]  root      /Count 99  (true visible total: 4)
        [7]  "Open"    /Count +9  -> OPEN,   2 children  (true: 2)
             [9]  "Kid A"
             [10] "Kid B"
        [8]  "Shut"    /Count -7  -> CLOSED, 1 child     (true: 1)
             [11] "Kid C"
    """
    objects, n = pages_doc(3, "/Outlines 6 0 R")
    assert n == 6, n
    objects[6] = root(7, 8, 99)
    objects[7] = item(b"Open", 6, nxt=8, first=9, last=10, count=9, dest="[3 0 R /Fit]")
    objects[8] = item(b"Shut", 6, prev=7, first=11, last=11, count=-7, dest="[4 0 R /Fit]")
    objects[9] = item(b"Kid A", 7, nxt=10, dest="[3 0 R /Fit]")
    objects[10] = item(b"Kid B", 7, prev=9, dest="[4 0 R /Fit]")
    objects[11] = item(b"Kid C", 8, dest="[5 0 R /Fit]")
    return serialize(objects)


# ---------------------------------------------------------------------
# named-dests.pdf
# ---------------------------------------------------------------------
def named_dests() -> bytes:
    """Both §12.3.2.3 namespaces, the /D wrapper, and an unresolvable name.

        [7]  catalog /Dests  (PDF 1.1 dict)  /LegacyIntro -> page 0
        [8]  /Names /Dests root -> /Kids [9]
        [9]  name-tree leaf: (tree-body) -> [4 0 R /Fit]
                             (tree-wrapped) -> << /D [5 0 R /FitV 40] >>
    """
    objects, n = pages_doc(3, "/Outlines 10 0 R /Dests 7 0 R /Names << /Dests 8 0 R >>")
    assert n == 6, n
    objects[7] = b"<< /LegacyIntro [3 0 R /XYZ 0 792 null] >>"
    objects[8] = b"<< /Kids [9 0 R] >>"
    objects[9] = (
        b"<< /Limits [(tree-action) (tree-wrapped)] /Names ["
        b"(tree-action) << /A << /S /GoTo /D [4 0 R /FitB] >> >> "
        b"(tree-body) [4 0 R /Fit] "
        b"(tree-wrapped) << /D [5 0 R /FitV 40] >>"
        b"] >>"
    )
    objects[10] = root(11, 16, 6)
    objects[11] = item(b"Legacy intro", 10, nxt=12, dest="/LegacyIntro")
    objects[12] = item(b"Tree body", 10, prev=11, nxt=13, dest="(tree-body)")
    objects[13] = item(b"Tree wrapped", 10, prev=12, nxt=14, dest="(tree-wrapped)")
    # §12.3.2.3 NOTE 2: the wrapper dictionary may carry a go-to ACTION.
    objects[14] = item(b"Tree action", 10, prev=13, nxt=15, dest="(tree-action)")
    # DEST-A1: a legacy-DICTIONARY key spelled as a STRING. The type says
    # "name tree"; only the legacy dictionary defines it.
    objects[15] = item(b"Crossed namespace", 10, prev=14, nxt=16, dest="(LegacyIntro)")
    objects[16] = item(b"Nowhere", 10, prev=15, dest="(nowhere)")
    return serialize(objects)


# ---------------------------------------------------------------------
# actions.pdf
# ---------------------------------------------------------------------
def actions() -> bytes:
    """Five §12.6 action dictionaries hanging off /A instead of /Dest."""
    objects, n = pages_doc(2, "/Outlines 5 0 R")
    assert n == 5, n
    objects[5] = root(6, 10, 5)
    objects[6] = item(
        b"GoTo local", 5, nxt=7, action="<< /S /GoTo /D [4 0 R /Fit] >>"
    )
    objects[7] = item(
        b"GoToR by number",
        5,
        prev=6,
        nxt=8,
        action="<< /S /GoToR /F (other.pdf) /D [7 /Fit] /NewWindow true >>",
    )
    objects[8] = item(
        b"GoToR by name",
        5,
        prev=7,
        nxt=9,
        action="<< /S /GoToR /F << /Type /Filespec /F (legacy.pdf) /UF (unicode.pdf) >> "
        "/D (remote-name) >>",
    )
    objects[9] = item(
        b"Web link",
        5,
        prev=8,
        nxt=10,
        action="<< /S /URI /URI (https://example.invalid/spec) >>",
    )
    objects[10] = item(
        b"Script",
        5,
        prev=9,
        action="<< /S /JavaScript /JS (app.alert\\(1\\);) >>",
    )
    return serialize(objects)


# ---------------------------------------------------------------------
# both-dest-and-a.pdf
# ---------------------------------------------------------------------
def both_dest_and_a() -> bytes:
    """/Dest and /A on one item, disagreeing, so precedence is provable."""
    objects, n = pages_doc(2, "/Outlines 5 0 R")
    assert n == 5, n
    objects[5] = root(6, 6, 1)
    objects[6] = item(
        b"Contested",
        5,
        dest="[3 0 R /Fit]",  # page index 0
        action="<< /S /GoTo /D [4 0 R /Fit] >>",  # page index 1
    )
    return serialize(objects)


# ---------------------------------------------------------------------
# broken-dests.pdf
# ---------------------------------------------------------------------
def broken_dests() -> bytes:
    """Three destinations that cannot reach a page index. None may vanish."""
    objects, n = pages_doc(2, "/Outlines 5 0 R")
    assert n == 5, n
    objects[5] = root(6, 8, 3)
    # 99 0 R is never defined, so the xref writes it as a free entry and
    # §7.3.10 makes it resolve to null.
    objects[6] = item(b"Dangling page", 5, nxt=7, dest="[99 0 R /Fit]")
    # Object 1 exists but is the catalog, not a page in the page tree.
    objects[7] = item(b"Not a page", 5, prev=6, nxt=8, dest="[1 0 R /Fit]")
    objects[8] = item(b"Empty array", 5, prev=7, dest="[]")
    return serialize(objects)


# ---------------------------------------------------------------------
# cycle.pdf
# ---------------------------------------------------------------------
def cycle() -> bytes:
    """Three distinct loops. A reader without a cycle guard never returns.

        [5]  root  /First 6  /Last 7
        [6]  "Ping"      /Next 7
        [7]  "Pong"      /Next 6   <-- sibling loop
        [8]  "Ouroboros" /First 8  <-- self-parent loop (reached from 6)
        [9]  "Backref"   /First 6  <-- child pointing at an ancestor's chain
    """
    objects, n = pages_doc(1, "/Outlines 5 0 R")
    assert n == 4, n
    objects[5] = root(6, 7, 2)
    objects[6] = item(b"Ping", 5, nxt=7, first=8, last=8, count=1, dest="[3 0 R /Fit]")
    objects[7] = item(b"Pong", 5, prev=6, nxt=6, dest="[3 0 R /Fit]")
    objects[8] = item(b"Ouroboros", 6, first=8, last=8, count=1, nxt=9)
    objects[9] = item(b"Backref", 6, prev=8, first=6, last=6, count=1)
    return serialize(objects)


# ---------------------------------------------------------------------
# deep.pdf
# ---------------------------------------------------------------------
def deep(levels: int = 40) -> bytes:
    """One chain `levels` deep, past the reader's nesting cap."""
    objects, n = pages_doc(1, "/Outlines 5 0 R")
    assert n == 4, n
    base = 6  # first item object number; 5 is the outline root
    objects[5] = root(base, base, levels)
    for depth in range(levels):
        num = base + depth
        parent = 5 if depth == 0 else num - 1
        child = num + 1 if depth + 1 < levels else None
        objects[num] = item(
            f"Level {depth}".encode("ascii"),
            parent,
            first=child,
            last=child,
            count=(levels - depth - 1) if child is not None else None,
            dest="[3 0 R /Fit]",
        )
    return serialize(objects)


# ---------------------------------------------------------------------
# titles.pdf
# ---------------------------------------------------------------------
def titles() -> bytes:
    """Four §7.9.2 text-string cases carried on /Title."""
    objects, n = pages_doc(1, "/Outlines 5 0 R")
    assert n == 4, n
    objects[5] = root(6, 9, 4)
    objects[6] = item(b"Plain ASCII", 5, nxt=7, dest="[3 0 R /Fit]")
    # UTF-16BE with the FE FF BOM: Greek "kappa-epsilon-phi" (chapter).
    utf16 = b"\xfe\xff\x03\xba\x03\xb5\x03\xc6"
    objects[7] = item(utf16, 5, prev=6, nxt=8, dest="[3 0 R /Fit]")
    # 0xA0 is EURO in PDFDocEncoding (Annex D.3), NOT a no-break space —
    # a reader that assumed Latin-1 gets this wrong and looks right doing it.
    objects[8] = item(b"\xa05 fee", 5, prev=7, nxt=9, dest="[3 0 R /Fit]")
    # 0xAD is one of the 24 UNDEFINED PDFDocEncoding codes: must be
    # disclosed as an inexact decode, never silently passed through.
    objects[9] = item(b"bad\xadbyte", 5, prev=8, dest="[3 0 R /Fit]")
    return serialize(objects)


# ---------------------------------------------------------------------
# no-outline.pdf
# ---------------------------------------------------------------------
def no_outline() -> bytes:
    """A catalog with no /Outlines: empty, not an error."""
    objects, _ = pages_doc(1)
    return serialize(objects)


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    files = {
        "basic-tree.pdf": basic_tree(),
        "lying-counts.pdf": lying_counts(),
        "named-dests.pdf": named_dests(),
        "actions.pdf": actions(),
        "both-dest-and-a.pdf": both_dest_and_a(),
        "broken-dests.pdf": broken_dests(),
        "cycle.pdf": cycle(),
        "deep.pdf": deep(),
        "titles.pdf": titles(),
        "no-outline.pdf": no_outline(),
    }
    for name, data in files.items():
        (OUT_DIR / name).write_bytes(data)
        print(f"wrote {name} ({len(data)} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
