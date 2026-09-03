#!/usr/bin/env python3
"""Generate the synthetic optional-content (layer / OCG) fixtures.

WHY THIS EXISTS
---------------
``crates/pdfcer-core/src/layers.rs`` reads ISO 32000-1:2008 §8.11 —
optional content groups, the catalog ``/OCProperties`` dictionary, and
the default configuration dictionary ``/D`` (Tables 98, 99, 100, 101).
Almost every claim that reader makes is about a structure no authoring
tool will produce on request:

* an OCG named in ``/D /Order`` (or in ``/OFF``, or by an annotation's
  ``/OC``) that is **absent** from ``/OCProperties /OCGs``, which
  §8.11.4.2 says shall list every OCG in the document;
* an ``/Order`` array that reaches itself through an indirect reference,
  which is a well-formed *file* describing an infinite tree;
* an ``/Order`` nested forty levels deep;
* an OCG appearing in **two** ``/RBGroups`` inner arrays at once;
* ``/BaseState /OFF`` in the ``/D`` configuration, which Table 101 says
  *shall* be ``ON`` there — and which real producers write anyway;
* an OCG dictionary with **no** ``/Name``, though Table 98 marks it
  Required.

Acrobat, Illustrator and every CAD exporter emit only the well-formed
subset. A reader tested against their output alone is a reader whose
first malformed file hangs it, or silently loses a layer.

``docs/LEGAL.md`` §5 permits only synthetic or rights-cleared PDFs under
``fixtures/``. Every byte this script writes is constructed here, from
nothing, with **no PDF library behind it** — the same discipline as the
sibling ``tools/gen-*-fixtures.py`` scripts. That is not merely a
licensing convenience. A fixture produced *through* a PDF library
inherits that library's normalizations, and a library that refuses to
write ``/Order 20 0 R`` where object 20 contains itself cannot express
the very defect the cycle test is about.

All files use a **classic** cross-reference table (§7.5.4) and US-Letter
(612x792) pages. Page content streams appear only where a fixture needs a
resource dictionary to hang an OCG off; the optional-content structure is
the whole subject, and painted marks would only add bytes no test reads.

WHAT IT WRITES
--------------
``fixtures/synthetic/layers/``

``basic-layers.pdf``
    The happy path. Four well-formed OCGs, all four registered in
    ``/OCProperties /OCGs``, ``/D`` carrying a flat ``/Order`` that lists
    only **three** of them, and ``/OFF`` naming one. Pins: default
    visibility comes from ``/OFF``; the unlisted fourth group is still a
    real layer but is "presented in no user interface" per Table 101;
    a ``/Name`` written as UTF-16BE (§7.9.2.2) decodes, and one written
    in PDFDocEncoding with a byte that is **not** Latin-1 decodes
    differently from a naive Latin-1 reading.

``nested-order.pdf``
    ``/Order`` exercising every element form Table 101 permits, with the
    group names chosen in **reverse alphabetical order** relative to
    their declared position. A reader that sorts by name, or that
    flattens the tree, produces a visibly different document.

``unregistered-ocg.pdf``
    The central malformation. ``/OCProperties /OCGs`` lists exactly one
    group; four more are reachable — from ``/D /Order``, from ``/D
    /OFF``, from an annotation's ``/OC``, and from a page's
    ``/Resources /Properties`` (the ``BDC /OC`` landing point, §8.11.3.2
    + §14.6.2) — and a sixth sits behind a form XObject's ``/OC``
    (§8.11.3.3) inside a *nested* resource dictionary. Every one of them
    must appear in the listing marked as unregistered, never dropped.

``radio-locked.pdf``
    ``/D /RBGroups`` with two inner arrays that **share a member**, plus
    ``/Locked`` naming a group that is itself in a radio group. Pins that
    radio membership is reported before a caller toggles anything, and
    that an overlapping member is disclosed rather than silently assigned
    to whichever array was scanned first.

``basestate-off.pdf``
    ``/D << /BaseState /OFF /ON [...] >>`` — Table 101 says a ``/D``
    configuration's ``/BaseState``, if present, *shall* be ``ON``. Real
    files disagree. Pins that pdfcer follows the initialisation order the
    standard gives (base state sets all groups, then ``/ON``/``/OFF``
    override) rather than the standard's *shall*, and discloses the
    violation instead of correcting it silently.

``order-cycle.pdf``
    Three structural hazards in one ``/Order``: an indirect array that
    contains **itself**, a two-array mutual loop, and a forty-level
    nesting chain. A reader without a cycle guard and a depth cap does
    not fail here — it hangs, or overflows its stack.

``ocmd-membership.pdf``
    An annotation whose ``/OC`` is an **OCMD** (Table 99) rather than a
    bare OCG, with two members, one registered and one not, plus an OCMD
    with an empty ``/OCGs`` (the spec's explicit "no effect" case) and
    one whose ``/OCGs`` is a single dictionary rather than an array.

``malformed-groups.pdf``
    Registered ``/OCGs`` entries that are not usable groups: one with no
    ``/Name`` at all (Table 98 Required), one whose ``/Name`` is a number,
    one that is a direct dictionary rather than an indirect reference
    (so it has no identity to toggle), one naming an object that does not
    exist, and one carrying ``/Intent /Design`` only — which a
    View-configured reader may legitimately ignore.

``no-layers.pdf``
    One page, no ``/OCProperties`` in the catalog. §8.11.4.2: a reader
    "shall ignore" all optional content when it is absent. The empty-not-
    error case, and overwhelmingly the common one.
"""

from pathlib import Path

OUT_DIR = Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "layers"

PAGE_W = 612
PAGE_H = 792


# --------------------------------------------------------------------------
# Byte-level plumbing
# --------------------------------------------------------------------------


def serialize(objects: dict[int, bytes]) -> bytes:
    """Lay out `objects` into a complete classic-xref file (§7.5.4).

    Entry format is exactly 20 bytes: ten digits, a space, five digits, a
    space, the keyword, a two-byte EOL -- written longhand so the byte
    count is visible. Object numbers with no body are emitted as **free**
    entries, which is what lets a fixture reference ``99 0 R`` and have it
    genuinely dangle (§7.3.10, "not an error ... treated as a reference to
    the null object") rather than be a parse error.

    Copied deliberately, not imported, from ``gen-outline-fixtures.py``:
    these generators are standalone by convention so that editing one
    corpus cannot silently reshape another's bytes.
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

    Backslash and both parentheses are escaped; every other byte is
    emitted as-is, **including bytes above 0x7F**. That last part is
    deliberate: ``basic-layers.pdf`` needs a raw 0xA0 byte to reach the
    text-string decoder unchanged (0xA0 is EURO in PDFDocEncoding, not the
    no-break space a Latin-1 reader would produce), and a "helpful" hex
    fallback would hide the very byte the test is about.
    """
    body = raw.replace(b"\\", b"\\\\").replace(b"(", b"\\(").replace(b")", b"\\)")
    return b"(" + body + b")"


def utf16be(text: str) -> bytes:
    """A §7.9.2.2 text string in its UTF-16BE form, BOM included."""
    return b"\xfe\xff" + text.encode("utf-16-be")


def stream(dict_body: str, data: bytes) -> bytes:
    """A stream object (§7.3.8) with a correct ``/Length``.

    ``/Length`` is written as a direct integer rather than an indirect
    reference. Both are legal; the direct form keeps each fixture's object
    numbering readable, and the indirect form is already exercised by
    other corpora.
    """
    return (
        f"<< {dict_body} /Length {len(data)} >>\nstream\n".encode("ascii")
        + data
        + b"\nendstream"
    )


def ocg(name: bytes, extra: str = "") -> bytes:
    """A minimal Table 98 optional content group dictionary.

    ``/Type /OCG`` and ``/Name`` are the two Required entries. `name` is
    already-serialized string-object bytes (see :func:`pdf_string`), not a
    Python string, because several fixtures need byte sequences no Python
    string round-trips cleanly.
    """
    return b"<< /Type /OCG /Name " + name + f" {extra} >>".encode("ascii")


def pages_doc(
    count: int,
    catalog_extra: str = "",
    page_extra: dict[int, str] | None = None,
) -> tuple[dict[int, bytes], int]:
    """A `count`-page skeleton.

    Object 1 is the catalog, object 2 the page-tree root, objects
    ``3 .. 3+count-1`` the pages. Returns the object table and the first
    free object number, so each fixture lays its optional-content objects
    out above the pages without hard-coding arithmetic that breaks the
    moment a page is added.

    `page_extra` maps a **0-based page index** to extra dictionary text
    for that page -- how the fixtures attach ``/Annots`` and
    ``/Resources``.
    """
    page_extra = page_extra or {}
    kids = " ".join(f"{3 + i} 0 R" for i in range(count))
    objects: dict[int, bytes] = {
        1: f"<< /Type /Catalog /Pages 2 0 R {catalog_extra} >>".encode("ascii"),
        2: (
            f"<< /Type /Pages /Kids [{kids}] /Count {count} "
            f"/MediaBox [0 0 {PAGE_W} {PAGE_H}] /Resources << >> >>"
        ).encode("ascii"),
    }
    for i in range(count):
        extra = page_extra.get(i, "")
        objects[3 + i] = f"<< /Type /Page /Parent 2 0 R {extra} >>".encode("ascii")
    return objects, 3 + count


# --------------------------------------------------------------------------
# The fixtures
# --------------------------------------------------------------------------


def basic_layers() -> bytes:
    """Four well-formed groups; three ordered, one hidden by default.

    Object map (after the 1 catalog / 2 pages / 3 page skeleton):

    ==== ===============================================================
    obj  content
    ==== ===============================================================
    4    OCG "Dimensions"        -- ordered, ON
    5    OCG "Hidden Notes"      -- ordered, in ``/OFF``
    6    OCG UTF-16BE "κεφ" -- ordered, ON
    7    OCG PDFDocEncoding with a raw 0xA0 byte -- registered but **not**
         in ``/Order``
    ==== ===============================================================

    Object 7 is the load-bearing one for two separate claims at once. Its
    name byte 0xA0 is EURO in PDFDocEncoding (Annex D.3) and NO-BREAK
    SPACE in Latin-1, so a decoder that guesses Latin-1 produces a
    plausible, wrong, silent answer. And its absence from ``/Order``
    exercises Table 101's "groups not listed are not presented" rule
    without making it disappear from the document.
    """
    objects, nxt = pages_doc(
        1,
        catalog_extra=(
            "/OCProperties << /OCGs [4 0 R 5 0 R 6 0 R 7 0 R] "
            "/D << /Name (Default) /Order [4 0 R 5 0 R 6 0 R] /OFF [5 0 R] "
            "/ListMode /AllPages >> >>"
        ),
    )
    objects[4] = ocg(pdf_string(b"Dimensions"))
    objects[5] = ocg(pdf_string(b"Hidden Notes"))
    objects[6] = ocg(pdf_string(utf16be("κεφ")))
    objects[7] = ocg(pdf_string(b"\xa05 tier"))
    assert nxt == 4
    return serialize(objects)


def nested_order() -> bytes:
    """``/Order`` with a label, a sub-tree under a group, and a bare nest.

    The declared order is::

        /Order [ (Sheet metal) [ 4 0 R [ 5 0 R 6 0 R ] ] 7 0 R [ 8 0 R ] ]

    which reads as: a non-selectable label "Sheet metal" heading a
    sub-tree; inside it group ZULU ("4") followed by a nested array whose
    contents are ZULU's children YANKEE and XRAY; then, back at the top
    level, group WHISKEY; then an unlabelled nested array holding VICTOR.

    Names run **Z, Y, X, W, V in declared order**, so any reader that
    sorts alphabetically, or that flattens the nesting, or that attaches
    the ``[5 0 R 6 0 R]`` sub-array to the wrong parent, produces an
    order no test can mistake for the right one.
    """
    objects, _ = pages_doc(
        1,
        catalog_extra=(
            "/OCProperties << /OCGs [4 0 R 5 0 R 6 0 R 7 0 R 8 0 R] "
            "/D << /Order [(Sheet metal) [4 0 R [5 0 R 6 0 R]] 7 0 R [8 0 R]] >> >>"
        ),
    )
    objects[4] = ocg(pdf_string(b"ZULU"))
    objects[5] = ocg(pdf_string(b"YANKEE"))
    objects[6] = ocg(pdf_string(b"XRAY"))
    objects[7] = ocg(pdf_string(b"WHISKEY"))
    objects[8] = ocg(pdf_string(b"VICTOR"))
    return serialize(objects)


def unregistered_ocg() -> bytes:
    """Five groups reachable, one registered. The §8.11.4.2 violation.

    ``/OCProperties /OCGs`` names **only** object 4. The other five reach
    the document by every other route the standard defines:

    ==== ===============================================================
    obj  how it is reachable, and from where
    ==== ===============================================================
    4    ``/OCGs`` **and** ``/Order``  -- the one registered group
    5    ``/D /Order`` only
    6    ``/D /OFF`` only -- so it is default-OFF while being unregistered
    7    an annotation's ``/OC`` (§8.11.3.3) on page 1
    8    the page's ``/Resources /Properties /oc1`` (§8.11.3.2 marked
         content, §14.6.2's named-resource requirement)
    9    a form XObject's ``/OC``, where the XObject is reached through
         the page's ``/Resources /XObject /Fm0`` -- and the XObject
         carries its **own** ``/Resources`` holding a second nested form
         (object 11) whose ``/OC`` is object 10
    10   only via object 11's ``/OC`` -- one resource level deeper than
         object 9, so a scanner that does not recurse loses exactly this
         one and nothing else
    ==== ===============================================================

    §8.11.4.2 says ``/OCProperties`` "shall be present if the file
    contains any optional content" and that every OCG "shall be included"
    in ``/OCGs``. It states no reader behaviour for a file that breaks
    that. pdfcer's choice -- report the group, flag it as unregistered --
    is therefore a disclosure, not a conformance verdict, and this fixture
    is the executable statement of it.
    """
    objects, _ = pages_doc(
        1,
        catalog_extra=(
            "/OCProperties << /OCGs [4 0 R] "
            "/D << /Order [4 0 R 5 0 R] /OFF [6 0 R] >> >>"
        ),
        page_extra={
            0: (
                "/Annots [12 0 R] "
                "/Resources << /Properties << /oc1 8 0 R >> "
                "/XObject << /Fm0 13 0 R >> >>"
            )
        },
    )
    for num, name in (
        (4, b"Registered"),
        (5, b"Order only"),
        (6, b"Off only"),
        (7, b"Annotation only"),
        (8, b"Marked content only"),
        (9, b"XObject only"),
        (10, b"Nested XObject only"),
    ):
        objects[num] = ocg(pdf_string(name))
    # A /Square annotation whose entire visibility hangs on object 7
    # (§8.11.3.3: an annotation with an /OC is visible only if the flags
    # permit AND the group says so).
    objects[12] = (
        b"<< /Type /Annot /Subtype /Square /Rect [72 72 200 200] /F 4 /OC 7 0 R >>"
    )
    # Form XObject (§8.10.1) carrying /OC and its own /Resources, which
    # hold a second form one level deeper.
    objects[13] = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 100 100] /OC 9 0 R "
        "/Resources << /XObject << /Fm1 11 0 R >> >>",
        b"/Fm1 Do",
    )
    objects[11] = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 50 50] /OC 10 0 R "
        "/Resources << >>",
        b"",
    )
    return serialize(objects)


def radio_locked() -> bytes:
    """Overlapping ``/RBGroups`` plus a ``/Locked`` member of one of them.

    ``/RBGroups [[4 0 R 5 0 R] [5 0 R 6 0 R]]`` puts object 5 in **two**
    radio-button arrays at once. Table 101 defines a radio-button array as
    a set in which at most one member is ON; it does not say what a reader
    does when the sets intersect, and an intersecting member makes the
    two constraints jointly unsatisfiable in the obvious way (turning 4 on
    forces 5 off, which says nothing about 6, while turning 6 on forces 5
    off again). pdfcer therefore reports the **whole** ``/RBGroups``
    structure and, in the per-layer convenience field, the first array a
    group belongs to -- with the overlap counted in the diagnostics, so a
    caller cannot mistake "first" for "only".

    ``/Locked [4 0 R]`` then locks a group that is *also* in a radio
    array, which is the combination a toggling UI must get right: the
    radio semantics say "turning 5 on turns 4 off", and the lock says "4's
    state cannot be changed through the UI".
    """
    objects, _ = pages_doc(
        1,
        catalog_extra=(
            "/OCProperties << /OCGs [4 0 R 5 0 R 6 0 R 7 0 R] "
            "/D << /Order [4 0 R 5 0 R 6 0 R 7 0 R] "
            "/RBGroups [[4 0 R 5 0 R] [5 0 R 6 0 R]] "
            "/Locked [4 0 R] >> >>"
        ),
    )
    objects[4] = ocg(pdf_string(b"Locked and radio"))
    objects[5] = ocg(pdf_string(b"In both radio groups"))
    objects[6] = ocg(pdf_string(b"Second radio group"))
    objects[7] = ocg(pdf_string(b"No radio group"))
    return serialize(objects)


def basestate_off() -> bytes:
    """``/D /BaseState /OFF`` -- which Table 101 says shall not happen.

    Table 101 on ``/BaseState``: "In the default configuration
    dictionary, the value of this entry shall be ON." This file sets it to
    ``OFF`` anyway, with ``/ON [5 0 R]`` re-enabling exactly one group.

    The correct reading follows the initialisation order the same table
    gives -- base state sets **all** groups, then ``/ON`` and ``/OFF``
    override per group -- so objects 4 and 6 are OFF and object 5 is ON.
    A reader that honours the *shall* by pretending ``/BaseState`` were
    ``ON`` reports the exact inverse for two of the three groups, and
    would show a CAD drawing with every layer lit that the author shipped
    dark.

    ``/OFF [6 0 R]`` is also present and is **redundant** under
    ``BaseState /OFF`` (the table says so in as many words). It is here so
    that the redundant-but-legal case is covered rather than assumed
    harmless.
    """
    objects, _ = pages_doc(
        1,
        catalog_extra=(
            "/OCProperties << /OCGs [4 0 R 5 0 R 6 0 R] "
            "/D << /BaseState /OFF /ON [5 0 R] /OFF [6 0 R] "
            "/Order [4 0 R 5 0 R 6 0 R] >> >>"
        ),
    )
    objects[4] = ocg(pdf_string(b"Off by base state"))
    objects[5] = ocg(pdf_string(b"On by override"))
    objects[6] = ocg(pdf_string(b"Off twice over"))
    return serialize(objects)


def order_cycle(levels: int = 40) -> bytes:
    """Self-reference, mutual reference, and a nesting chain past the cap.

    ``/Order`` is object 20, and object 20 contains ``20 0 R``. Following
    it naively is not slow -- it does not terminate. Objects 21 and 22
    then reference each other, which a naive "did I just see this exact
    object?" guard misses but a visited-set guard catches. Object 23
    starts a chain of `levels` singly-nested indirect arrays, ending in a
    real group, so the depth cap is exercised by a structure that is
    otherwise entirely well-formed.

    A reader without both guards does not report a wrong answer here. It
    hangs, or it overflows its stack -- and pdfcer-core's panic-free policy
    (``lib.rs``) treats a stack overflow on untrusted input exactly as it
    treats an ``unwrap``.
    """
    objects, _ = pages_doc(
        1,
        catalog_extra=("/OCProperties << /OCGs [4 0 R 5 0 R 6 0 R] /D << /Order 20 0 R >> >>"),
    )
    objects[4] = ocg(pdf_string(b"Reachable before the loop"))
    objects[5] = ocg(pdf_string(b"Behind the mutual loop"))
    objects[6] = ocg(pdf_string(b"At the bottom of the deep chain"))
    objects[20] = b"[4 0 R 20 0 R 21 0 R 23 0 R]"
    objects[21] = b"[5 0 R 22 0 R]"
    objects[22] = b"[21 0 R]"
    # A chain of `levels` nested indirect arrays: 23 -> 24 -> ... -> group.
    for depth in range(levels):
        num = 23 + depth
        if depth == levels - 1:
            objects[num] = b"[6 0 R]"
        else:
            objects[num] = f"[{num + 1} 0 R]".encode("ascii")
    return serialize(objects)


def ocmd_membership() -> bytes:
    """Annotations whose ``/OC`` is an OCMD (Table 99), in four shapes.

    ==== ===============================================================
    obj  OCMD shape
    ==== ===============================================================
    10   ``/OCGs [4 0 R 7 0 R]`` -- one registered member, one not. Both
         are real layers and both must be listed.
    11   ``/OCGs []``            -- the spec's explicit "no effect" case:
         an OCMD with no determinable members leaves content visible.
    12   ``/OCGs 5 0 R``         -- a **single dictionary** rather than an
         array, which Table 99 permits ("dictionary or array") and which
         a reader that assumes an array silently reads as no members.
    13   ``/OCGs [4 0 R] /P /AllOff`` -- a non-default visibility policy,
         present so the listing is proved to be about *membership* and
         not about evaluating the policy (which is
         ``annot::oc_is_hidden``'s job, not this module's).
    ==== ===============================================================
    """
    objects, _ = pages_doc(
        1,
        catalog_extra=(
            "/OCProperties << /OCGs [4 0 R 5 0 R] /D << /Order [4 0 R 5 0 R] >> >>"
        ),
        page_extra={0: "/Annots [20 0 R 21 0 R 22 0 R 23 0 R]"},
    )
    objects[4] = ocg(pdf_string(b"Registered A"))
    objects[5] = ocg(pdf_string(b"Registered B"))
    objects[7] = ocg(pdf_string(b"Unregistered OCMD member"))
    objects[10] = b"<< /Type /OCMD /OCGs [4 0 R 7 0 R] >>"
    objects[11] = b"<< /Type /OCMD /OCGs [] >>"
    objects[12] = b"<< /Type /OCMD /OCGs 5 0 R >>"
    objects[13] = b"<< /Type /OCMD /OCGs [4 0 R] /P /AllOff >>"
    for i, ocmd in enumerate((10, 11, 12, 13)):
        objects[20 + i] = (
            f"<< /Type /Annot /Subtype /Square /Rect [72 72 200 200] "
            f"/F 4 /OC {ocmd} 0 R >>"
        ).encode("ascii")
    return serialize(objects)


def malformed_groups() -> bytes:
    """Registered ``/OCGs`` entries that are not usable groups.

    ==== ===============================================================
    obj  defect
    ==== ===============================================================
    4    no ``/Name``            -- Table 98 marks it **Required**. A
         panel must not invent a name, and must not silently show a blank
         row as though the author had named it "".
    5    ``/Name 42``            -- present, wrong type. Same rule.
    6    fine, but ``/Intent /Design`` only -- §8.11.2.3: a reader
         configured for ``View`` may legitimately ignore it, so a panel
         needs to know before it shows it as an ordinary toggle.
    7    ``/Intent [/View /Design]`` -- the array form, participating.
    --   ``99 0 R`` in ``/OCGs`` names an object the file does not define
         (§7.3.10 -> null). Not an error; not a layer either.
    --   a **direct** dictionary inside ``/OCGs`` -- syntactically fine,
         but it has no object identity, so nothing can toggle it and
         nothing can point ``/OFF`` at it.
    ==== ===============================================================
    """
    objects, _ = pages_doc(
        1,
        catalog_extra=(
            "/OCProperties << /OCGs [4 0 R 5 0 R 6 0 R 7 0 R 99 0 R "
            "<< /Type /OCG /Name (Direct, unaddressable) >>] "
            "/D << /Order [4 0 R 5 0 R 6 0 R 7 0 R] >> >>"
        ),
    )
    objects[4] = b"<< /Type /OCG >>"
    objects[5] = b"<< /Type /OCG /Name 42 >>"
    objects[6] = ocg(pdf_string(b"Design intent only"), "/Intent /Design")
    objects[7] = ocg(pdf_string(b"Both intents"), "/Intent [/View /Design]")
    return serialize(objects)


def painted_layers() -> bytes:
    """The first fixture whose layers are actually PAINTED (§8.11.3.2).

    Every other fixture in this directory exercises the *enumerator* --
    ``/OCProperties`` structure, name decoding, ``/Order`` hazards. None
    of them draws anything, because until 2026-08-10 pdfcer honoured
    optional content only on an **annotation's** ``/OC`` entry and
    content-stream ``BDC``/``EMC`` was deferred. A fixture that paints
    through ``BDC`` had nothing to pin.

    This one paints four marks and turns two of them off, so the file
    answers "is content-stream optional content honoured?" by LOOKING at
    it -- which is the check an operator can perform and a diff cannot:

    ==== ======================= ============ ==========================
    obj  layer                   state        mark
    ==== ======================= ============ ==========================
    4    "Visible Box"           ON           filled square, lower-left
    5    "Hidden Box"            ``/OFF``     filled square, lower-right
    6    "Clip Only"             ``/OFF``     NO mark; sets a clip
    7    "Nested Inner"          ON           inside 5's section
    ==== ======================= ============ ==========================

    Three separate claims, one file:

    * obj 5's square must not appear.
    * obj 7 is ON but sits *inside* obj 5's hidden section, and must not
      appear either -- visibility is inherited, and an inner ``EMC``
      restores "hidden", not "visible" (Sec. 8.11.3.1).
    * obj 6's section is hidden but establishes a **clip** that the
      unlayered content after it must still obey. Sec. 8.11.3.1 says
      hidden content "shall not be drawn"; it does not say the graphics
      state it sets is discarded. If a renderer skips the clip along
      with the paint, the final full-width bar spills past x=300 and the
      page is visibly wrong in a way no counter reports.

    The unlayered bar is drawn LAST and full width on purpose: it is the
    only mark that must appear at partial width, so the fixture fails
    visibly rather than by an assertion.
    """
    objects, nxt = pages_doc(
        1,
        catalog_extra=(
            "/OCProperties << /OCGs [4 0 R 5 0 R 6 0 R 7 0 R] "
            "/D << /Name (Default) /Order [4 0 R 5 0 R 6 0 R 7 0 R] "
            "/OFF [5 0 R 6 0 R] >> >>"
        ),
        page_extra={
            0: (
                "/Contents 8 0 R /Resources << /Properties << "
                "/L1 4 0 R /L2 5 0 R /L3 6 0 R /L4 7 0 R >> >>"
            )
        },
    )
    objects[4] = ocg(pdf_string(b"Visible Box"))
    objects[5] = ocg(pdf_string(b"Hidden Box"))
    objects[6] = ocg(pdf_string(b"Clip Only"))
    objects[7] = ocg(pdf_string(b"Nested Inner"))
    content = b"""/OC /L1 BDC 0 0 0 rg 60 60 120 120 re f EMC
/OC /L2 BDC 0 0 0 rg 400 60 120 120 re f
  /OC /L4 BDC 0 0 0 rg 400 220 120 120 re f EMC
EMC
/OC /L3 BDC 0 0 300 792 re W n EMC
0.5 g 0 600 612 60 re f
"""
    objects[8] = stream("", content)
    assert nxt == 4
    return serialize(objects)


def on_off_contradiction() -> bytes:
    """A group listed in **both** ``/D /ON`` and ``/D /OFF`` (decision 038).

    Nothing forbids this. Both Table 101 rows are worded as obligations
    on the *reader* ("whose state **shall be** set to..."), not as
    restrictions on the writer, and SS8.11 contains no ``shall not`` about
    array membership. So the file is conforming and says two things.

    The resolution is not a coin toss and this fixture pins it. Table
    101's own ``ON`` row says the array is *redundant* when ``/BaseState``
    is ``ON``, and an array carrying no information cannot override
    anything -- so the **opposite** array decides, which is exactly what
    SS8.11.4.5 b) says. With the conforming ``/BaseState ON``, a
    both-listed group is **OFF**.

    Object 4 is in both arrays and must report hidden; object 5 is in
    neither and must report visible, so a reader that simply hid
    everything would fail too.

    The point of the fixture is the DISCLOSURE as much as the answer:
    ``contradictory_on_off_groups`` must be 1, because an operator
    looking at a layer that is off, in a document whose ``/ON`` array
    names it, cannot otherwise tell a correct resolution from a bug.
    """
    objects, nxt = pages_doc(
        1,
        catalog_extra=(
            "/OCProperties << /OCGs [4 0 R 5 0 R] "
            "/D << /Name (Default) /BaseState /ON /Order [4 0 R 5 0 R] "
            "/ON [4 0 R] /OFF [4 0 R] >> >>"
        ),
    )
    objects[4] = ocg(pdf_string(b"In both arrays"))
    objects[5] = ocg(pdf_string(b"In neither"))
    assert nxt == 4
    return serialize(objects)


def base_state_unchanged() -> bytes:
    """``/D /BaseState /Unchanged`` -- non-conforming, and empty.

    Table 101: *"If BaseState is present in the document's default
    configuration dictionary, its value shall be ON."* ``/Unchanged``
    violates that ``shall`` outright.

    It is also semantically empty in ``/D``. SS8.11.2.1 says states are not
    part of the document and are initialised when it opens, so at first
    open there is no prior state to leave unchanged. ``/Unchanged``
    exists for the *other* consumer of Table 101 -- an alternate
    ``/Configs`` configuration applied to an already-open document.

    pdfcer recovers by treating it as ``ON`` and processing ``/OFF``, so
    object 5 is hidden. That is Table 101's stated default and the only
    value ``/D`` was allowed to carry; the rival recovery ("leave
    everything as found, process no arrays") would make ``/OFF`` inert
    and paint every layer the author turned off.

    Pins the recovery AND its disclosure: ``base_state_unrecognised``
    must fire, because a reader following the *shall* and a reader
    following the file produce different pages here.
    """
    objects, nxt = pages_doc(
        1,
        catalog_extra=(
            "/OCProperties << /OCGs [4 0 R 5 0 R] "
            "/D << /Name (Default) /BaseState /Unchanged /Order [4 0 R 5 0 R] "
            "/OFF [5 0 R] >> >>"
        ),
    )
    objects[4] = ocg(pdf_string(b"Left alone"))
    objects[5] = ocg(pdf_string(b"In OFF"))
    assert nxt == 4
    return serialize(objects)


def base_state_off_unregistered() -> bytes:
    """``/BaseState /OFF`` **and** a group missing from ``/OCGs`` (decision 037).

    The one file that separates the two readings of Table 101's "all the
    optional content groups in a document", and it did not exist -- which
    is why the question shipped unanswered and had to be ruled on from
    the text alone.

    ``unregistered-ocg.pdf`` has unregistered groups but no
    ``/BaseState``; ``basestate-off.pdf`` has ``/BaseState /OFF`` but
    registers everything. Neither can tell the readings apart. This one
    has both conditions at once:

    * object 4 is registered in ``/OCGs`` and named in ``/ON``
    * object 5 is registered and named nowhere
    * object 6 is **not registered at all**, reachable only from the
      page's ``/Properties`` -- exactly the shape an editing tool leaves
      behind when it rewrites the registry and misses a group

    Under ``/BaseState /OFF``:

    * the **literal** reading ("every group in the document") hides
      object 6, since it is not in ``/ON``
    * the **registered-only** reading -- what pdfcer ships today, because
      the OFF set is enumerated from ``/OCGs`` -- reports it VISIBLE

    Both marks are painted through ``BDC``/``EMC`` so the difference is
    something an operator can SEE rather than something only a counter
    reports. Object 6's square is the whole experiment: if it is on the
    page, the reading is registered-only.

    Two ``shall``s are violated at once here (``/D``'s ``/BaseState``
    shall be ``ON``; every OCG shall be registered), which is precisely
    why the case is rare and why no fixture had cornered it before.
    """
    objects, nxt = pages_doc(
        1,
        catalog_extra=(
            "/OCProperties << /OCGs [4 0 R 5 0 R] "
            "/D << /Name (Default) /BaseState /OFF /Order [4 0 R 5 0 R] "
            "/ON [4 0 R] >> >>"
        ),
        page_extra={
            0: (
                "/Contents 7 0 R /Resources << /Properties << "
                "/Reg 4 0 R /RegOff 5 0 R /Unreg 6 0 R >> >>"
            )
        },
    )
    objects[4] = ocg(pdf_string(b"Registered, in ON"))
    objects[5] = ocg(pdf_string(b"Registered, not in ON"))
    objects[6] = ocg(pdf_string(b"Never registered"))
    content = b"""/OC /Reg BDC 0 0 0 rg 60 600 120 120 re f EMC
/OC /RegOff BDC 0 0 0 rg 240 600 120 120 re f EMC
/OC /Unreg BDC 0 0 0 rg 420 600 120 120 re f EMC
"""
    objects[7] = stream("", content)
    assert nxt == 4
    return serialize(objects)


def usage_auto_state() -> bytes:
    """``/AS`` usage application: a zoom-banded layer and a view-off one.

    The only fixture whose visible content depends on the MAGNIFICATION
    rather than on the file alone -- which is the whole point of
    SS8.11.4.4, and the reason it needs its own file: every other layer
    fixture has one right answer, and this one has three.

    ==== ===================== ======================================
    obj  layer                 behaviour
    ==== ===================== ======================================
    4    "Zoomed detail"       ``/Zoom << /min 2.0 /max 8.0 >>``
    5    "Hidden on view"      ``/View << /ViewState /OFF >>``
    6    "No usage"            no ``/Usage`` at all -- the control
    ==== ===================== ======================================

    Expected, and the reason each row exists:

    * below 2x: only obj 6 paints. Obj 4 is out of band, obj 5 is
      view-off.
    * at exactly 2.0: obj 4 JOINS it. ``min`` is inclusive.
    * at exactly 8.0: obj 4 leaves again. ``max`` is EXCLUSIVE, and this
      is the boundary an implementation using ``<=`` gets wrong at
      precisely one magnification.
    * obj 6 paints at every zoom, in every configuration. A group with no
      ``/Usage`` is "left unchanged", and a reader that treated an absent
      category as a recommendation of OFF would blank it -- so this
      square is what separates the two readings of the aggregation
      sentence.
    * obj 5 never paints while viewing, and DOES paint when the
      ``/D``-initial state is used, because SS8.11.4.5 forbids printing
      and aggregating applications from applying usage at all. Rendering
      this file with no magnification supplied is the print answer and
      must show it.

    Marks are painted through ``BDC``/``EMC`` so all of the above is
    something to look at rather than a counter to read.
    """
    objects, nxt = pages_doc(
        1,
        catalog_extra=(
            "/OCProperties << /OCGs [4 0 R 5 0 R 6 0 R] "
            "/D << /Name (Default) /Order [4 0 R 5 0 R 6 0 R] "
            "/AS [ << /Event /View /Category [/Zoom /View] "
            "/OCGs [4 0 R 5 0 R 6 0 R] >> ] >> >>"
        ),
        page_extra={
            0: (
                "/Contents 7 0 R /Resources << /Properties << "
                "/Zoomed 4 0 R /Hidden 5 0 R /Plain 6 0 R >> >>"
            )
        },
    )
    objects[4] = ocg(
        pdf_string(b"Zoomed detail"),
        extra="/Usage << /Zoom << /min 2.0 /max 8.0 >> >>",
    )
    objects[5] = ocg(
        pdf_string(b"Hidden on view"),
        extra="/Usage << /View << /ViewState /OFF >> >>",
    )
    objects[6] = ocg(pdf_string(b"No usage"))
    content = b"""/OC /Zoomed BDC 0 0 0 rg 60 600 120 120 re f EMC
/OC /Hidden BDC 0 0 0 rg 240 600 120 120 re f EMC
/OC /Plain BDC 0 0 0 rg 420 600 120 120 re f EMC
"""
    objects[7] = stream("", content)
    assert nxt == 4
    return serialize(objects)


def no_layers() -> bytes:
    """One page, no ``/OCProperties``.

    §8.11.4.2: if ``/OCProperties`` is absent, a conforming reader "shall
    ignore" every optional-content structure in the file. An empty layer
    listing with clean diagnostics is the whole correct answer, and this
    is the shape of the overwhelming majority of real PDFs.
    """
    objects, _ = pages_doc(1)
    return serialize(objects)


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    files = {
        "basic-layers.pdf": basic_layers(),
        "nested-order.pdf": nested_order(),
        "unregistered-ocg.pdf": unregistered_ocg(),
        "radio-locked.pdf": radio_locked(),
        "basestate-off.pdf": basestate_off(),
        "order-cycle.pdf": order_cycle(),
        "ocmd-membership.pdf": ocmd_membership(),
        "malformed-groups.pdf": malformed_groups(),
        "painted-layers.pdf": painted_layers(),
        "on-off-contradiction.pdf": on_off_contradiction(),
        "base-state-unchanged.pdf": base_state_unchanged(),
        "base-state-off-unregistered.pdf": base_state_off_unregistered(),
        "usage-auto-state.pdf": usage_auto_state(),
        "no-layers.pdf": no_layers(),
    }
    for name, data in files.items():
        (OUT_DIR / name).write_bytes(data)
        print(f"wrote {name} ({len(data)} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
