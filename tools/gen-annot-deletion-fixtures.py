#!/usr/bin/env python3
"""Byte-author the fixtures ``Pass 38.5``'s general annotation deletion needs.

WHY THESE FIXTURES EXIST
------------------------
``EditSession::delete_annotation`` is the first verb in pdfcer that removes
an annotation it did not author, and almost none of its behaviour is
about the annotation the operator named. Three cascades fire on OTHER
objects — a ``/Popup`` companion, every ``/IRT`` referrer, and the
appearance streams — and a fixture that carries a single lonely
annotation would let all three regress silently while every assertion
stayed green. That is **R162** (an assertion that cannot come out false)
in its most expensive form: the tests would look thorough.

So each file below is built to make a specific cascade *falsifiable*.

``annot/thread.pdf``
    One page, seven annotations, arranged so that deleting **one** of
    them has consequences for **five** others.

    ==== ============== ================================================
    Obj  Subtype        Role in the test
    ==== ============== ================================================
    4    ``/Square``    The **primary**. Carries ``/Popup 5 0 R``, its
                        own author/note, and a private ``/AP /N`` (11).
    5    ``/Popup``     4's window, with the Table 183 ``/Parent`` back
                        reference. §12.5.6.14: it "shall not appear
                        alone", so deleting 4 must take it.
    6    ``/Text``      A reply: ``/IRT 4 0 R`` **with** ``/RT /R``.
    7    ``/Text``      A reply: ``/IRT 4 0 R`` with **no** ``/RT`` at
                        all. Table 170's default value is ``R``, so a
                        reader that treats an absent ``/RT`` as "not a
                        reply" is wrong in the ORDINARY case — this
                        object is the one that catches it.
    8    ``/Square``    A ``/RT /Group`` **subordinate** of 4. §12.5.6.2
                        makes its own ``/Contents``/``/T``/``/M``/``/C``
                        group attributes a reader "shall ignore" while
                        the primary lives, so it must be counted apart
                        from 6 and 7.
    9    ``/Stamp``     Shares appearance stream 12 with object 10.
    10   ``/Stamp``     Shares appearance stream 12 with object 9.
    ==== ============== ================================================

    Objects 9 and 10 exist for one reason: **a shared appearance stream
    must survive the first of its users being deleted.** Referencing one
    form XObject from many annotations is entirely legal — §12.5.5 maps
    the same ``/BBox`` into a different ``/Rect`` per annotation, which
    is how a producer stamps "DRAFT" on forty pages with one stream —
    and a deletion that unconditionally removed its target's ``/AP``
    would blank the other thirty-nine. Without a second user in the
    fixture, "delete the /AP" and "delete the /AP if unshared" are
    indistinguishable and both pass.

    Objects 6 AND 7 rather than one reply, likewise: an implementation
    that only recognised an explicit ``/RT /R`` would still pass with
    object 6 alone.

``annot/certified-p3.pdf`` + ``annot/certified-p2-annot.pdf``
    A ``/P 3`` certification, and the corpus had **no** ``/P 3`` file
    before this one — ``/P 1`` (certified-*.pdf) and ``/P 2``
    (forms/certified-p2-form.pdf) were the whole range.

    ``/P 3`` is the value that separates ``annotation_deletion_refusal``
    from ``deletion_refusal``. Per ISO 32000-1 §12.8.2.2 Table 254,
    ``/P 3`` permits *"the same as for 2, as well as **annotation
    creation, deletion, and modification**"* — so on this document pdfcer
    must REFUSE to delete a form field and ALLOW deleting a comment.
    This is the first pdfcer operation any ``P`` value permits: until
    Pass 38.5 the strict gate was correct because, as
    ``SignatureCensus::forbids_structural_change``'s own doc comment put
    it, no ``P`` value's permitted list contained anything pdfcer could
    do.

    Each carries one deletable ``/Square`` annotation AND one form field,
    so both halves of the divergence are assertable on the same file
    rather than inferred across two.

    ``certified-p2-annot.pdf`` is the ``/P 3`` file with one digit
    changed — see :func:`build_certified`. It is not redundant with the
    existing ``forms/certified-p2-form.pdf``: that file contains **only
    widgets**, so pointing the annotation verb at it hits the widget
    refusal before the certification gate is ever reached, and proves
    nothing about ``/P``.

PROVENANCE
----------
100% byte-authored here — no PDF library is involved, so a fixture
cannot inherit a bug (or a normalisation) from the code it is meant to
test. Project rule 7 / ``LEGAL.md`` §5: synthetic or rights-cleared
only, never a downloaded real-world file.

Emits a **classic** cross-reference table (ISO 32000-1 §7.5.4), matching
every other ``tools/gen-*-fixtures.py`` in this tree.

USAGE
-----
    python tools/gen-annot-deletion-fixtures.py
"""

from pathlib import Path

OUT_DIR = Path("fixtures/synthetic/annot")


def stream_obj(dict_prefix: bytes, content: bytes) -> bytes:
    """A stream object: dict + /Length + the stream body."""
    return (
        dict_prefix
        + b" /Length %d >>\nstream\n" % len(content)
        + content
        + b"\nendstream"
    )


def assemble(objs: dict[int, bytes]) -> bytes:
    """Serialise a 1-based object table with a classic xref (§7.5.4).

    Object numbers must be contiguous from 1; the fixtures below are, and
    a gap would silently emit a free entry pointing at offset 0.
    """
    buf = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n"
    off: dict[int, int] = {}
    for n in sorted(objs):
        off[n] = len(buf)
        buf += b"%d 0 obj\n" % n + objs[n] + b"\nendobj\n"
    xref_at = len(buf)
    size = max(objs) + 1
    buf += b"xref\n0 %d\n0000000000 65535 f \n" % size
    for n in range(1, size):
        buf += b"%010d 00000 n \n" % off[n]
    buf += (
        b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n"
        % (size, xref_at)
    )
    return buf


def build_thread() -> bytes:
    """The reply/group/popup/shared-appearance fixture."""
    objs: dict[int, bytes] = {}
    objs[1] = b"<< /Type /Catalog /Pages 2 0 R >>"
    objs[2] = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"
    objs[3] = (
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] "
        b"/Resources << >> /Annots [4 0 R 5 0 R 6 0 R 7 0 R 8 0 R 9 0 R 10 0 R] >>"
    )
    # 4 — THE PRIMARY. Everything else in this file points at it.
    objs[4] = (
        b"<< /Type /Annot /Subtype /Square /Rect [20 220 120 280] /P 3 0 R "
        b"/T (Alice) /Contents (the primary comment) /M (D:20260808120000Z) "
        b"/Popup 5 0 R /AP << /N 11 0 R >> >>"
    )
    # 5 — its window. Table 183 /Parent is the back reference; pdfcer finds
    # the pair from the PARENT's /Popup, not from this, so a fixture that
    # only carried /Parent would not exercise the path under test.
    objs[5] = (
        b"<< /Type /Annot /Subtype /Popup /Rect [130 220 290 280] /P 3 0 R "
        b"/Parent 4 0 R /Open false >>"
    )
    # 6 — an EXPLICIT /RT /R reply.
    objs[6] = (
        b"<< /Type /Annot /Subtype /Text /Rect [20 180 40 200] /P 3 0 R "
        b"/T (Bob) /Contents (first reply) /IRT 4 0 R /RT /R >>"
    )
    # 7 — an IMPLICIT reply: no /RT at all, which Table 170 defaults to /R.
    objs[7] = (
        b"<< /Type /Annot /Subtype /Text /Rect [50 180 70 200] /P 3 0 R "
        b"/T (Carol) /Contents (second reply, no explicit /RT) /IRT 4 0 R >>"
    )
    # 8 — a GROUP SUBORDINATE. Its /T and /Contents are group attributes a
    # conforming reader must IGNORE while object 4 exists (§12.5.6.2).
    objs[8] = (
        b"<< /Type /Annot /Subtype /Square /Rect [20 120 120 160] /P 3 0 R "
        b"/T (Dave) /Contents (suppressed while the primary lives) "
        b"/IRT 4 0 R /RT /Group >>"
    )
    # 9 and 10 — two annotations, ONE appearance stream. The pair is the
    # whole reason this file has stamps in it.
    objs[9] = (
        b"<< /Type /Annot /Subtype /Stamp /Rect [20 40 100 90] /P 3 0 R "
        b"/Name /Draft /AP << /N 12 0 R >> >>"
    )
    objs[10] = (
        b"<< /Type /Annot /Subtype /Stamp /Rect [140 40 220 90] /P 3 0 R "
        b"/Name /Draft /AP << /N 12 0 R >> >>"
    )
    # 11 — owned solely by object 4, so deleting 4 must take it.
    objs[11] = stream_obj(
        b"<< /Type /XObject /Subtype /Form /BBox [0 0 100 60]",
        b"0 0 100 60 re S",
    )
    # 12 — shared by 9 and 10, so deleting either alone must NOT take it.
    objs[12] = stream_obj(
        b"<< /Type /XObject /Subtype /Form /BBox [0 0 80 50]",
        b"0 0 80 50 re S",
    )
    return assemble(objs)


def build_undeletable() -> bytes:
    """The two annotations ``delete_annotation`` must refuse.

    Both refusals come from the STANDARD rather than from pdfcer policy,
    and both are invisible: a locked annotation and a trap network look
    like any other annotation from a comments list, so an implementation
    that never read the flag or the subtype would behave identically on
    every document except the ones where it matters. Nothing in the
    fixtures above would catch that.

    Object 4 — ``/F 128``, Table 165 **bit 8 `Locked`**: *"do not allow
    the annotation to be deleted or its properties (including position
    and size) to be modified by the user."*

    Object 5 — ``/F 512``, Table 165 **bit 10 `LockedContents`**, whose
    own row says it *"does not restrict deletion"*. **This object is the
    falsifier**: an implementation that refused on any lock-shaped flag,
    or that got the bit numbering off by two, would refuse this one too,
    and the "Locked refuses" assertion alone cannot tell the difference.

    Object 6 — ``/TrapNet``, §12.5.6.21, which *"shall be the last
    element of the page's Annots array"*. Prepress output state, not
    markup. It IS last in ``/Annots`` here, as the clause requires.
    """
    objs: dict[int, bytes] = {}
    objs[1] = b"<< /Type /Catalog /Pages 2 0 R >>"
    objs[2] = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"
    objs[3] = (
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] "
        b"/Resources << >> /Annots [4 0 R 5 0 R 6 0 R] >>"
    )
    objs[4] = (
        b"<< /Type /Annot /Subtype /Square /Rect [20 120 120 180] /P 3 0 R "
        b"/F 128 /T (Alice) /Contents (locked: bit 8) >>"
    )
    objs[5] = (
        b"<< /Type /Annot /Subtype /Square /Rect [140 120 240 180] /P 3 0 R "
        b"/F 512 /T (Bob) /Contents (LockedContents: bit 10, deletable) >>"
    )
    objs[6] = (
        b"<< /Type /Annot /Subtype /TrapNet /Rect [0 0 300 200] /P 3 0 R "
        b"/Version [] /AnnotStates [] >>"
    )
    return assemble(objs)


def build_certified(permission: int) -> bytes:
    """A certified fixture at ``/P permission``, otherwise identical.

    TWO FILES FROM ONE BUILDER, AND THE SAMENESS IS THE POINT. The pair
    differs in exactly one integer, so a test asserting "deletion is
    permitted at 3 and refused at 2" cannot be passing because of some
    other difference between two hand-written files. It also makes the
    ``/P 2`` file the falsifier for the ``/P 3`` one: without it,
    "permitted at ``/P 3``" is equally consistent with the annotation
    verb never consulting the certification at all — R162.
    """
    objs: dict[int, bytes] = {}
    objs[1] = (
        b"<< /Type /Catalog /Pages 2 0 R /Perms << /DocMDP 7 0 R >> "
        b"/AcroForm << /Fields [5 0 R] /DA (/Helv 0 Tf 0 g) >> >>"
    )
    objs[2] = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"
    objs[3] = (
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] "
        b"/Resources << >> /Annots [4 0 R 5 0 R] >>"
    )
    # The annotation whose deletion /P 3 permits.
    objs[4] = (
        b"<< /Type /Annot /Subtype /Square /Rect [20 120 120 180] /P 3 0 R "
        b"/T (Reviewer) /Contents (a comment on a certified document) "
        b"/AP << /N 6 0 R >> >>"
    )
    # The form field whose deletion it does NOT: /P 3 is /P 2 plus
    # annotations, and a field is not an annotation change.
    objs[5] = (
        b"<< /FT /Tx /T (FullName) /TU (Full name) /Subtype /Widget "
        b"/Rect [20 40 250 62] /P 3 0 R >>"
    )
    objs[6] = stream_obj(
        b"<< /Type /XObject /Subtype /Form /BBox [0 0 100 60]",
        b"0 0 100 60 re S",
    )
    # THE POINT OF THE FIXTURE. Structural placeholders only: the guards
    # under test read census-visible structure and verify no cryptography,
    # and a fixture carrying a real signature would test a claim pdfcer does
    # not make while adding a key nobody can rotate.
    objs[7] = (
        b"<< /Type /Sig /Filter /Adobe.PPKLite /ByteRange [0 1 2 3] "
        b"/Reference [ << /TransformMethod /DocMDP "
        b"/TransformParams << /P %d >> >> ] >>" % permission
    )
    return assemble(objs)


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    for name, data in (
        ("thread.pdf", build_thread()),
        ("certified-p3.pdf", build_certified(3)),
        ("certified-p2-annot.pdf", build_certified(2)),
        ("undeletable.pdf", build_undeletable()),
    ):
        path = OUT_DIR / name
        path.write_bytes(data)
        print(f"wrote {path} ({len(data)} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
