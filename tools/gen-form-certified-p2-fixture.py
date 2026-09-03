#!/usr/bin/env python3
"""Byte-author ``forms/certified-p2-form.pdf``.

WHY THIS FIXTURE EXISTS
-----------------------
``EditSession::fill_refusal`` and ``EditSession::deletion_refusal`` use
**different** certification gates, and the difference is load-bearing for
the GUI: filling takes the ``/P``-aware gate, deletion takes the strict
one, so **there are documents where pdfcer offers filling and refuses
deletion.**

Every certification fixture in the corpus before this one used
``/P 1`` — "no changes permitted" — which refuses *both* operations. A
test written against ``/P 1`` passes whether or not the two gates differ
at all, and would go on passing if someone collapsed ``deletion_refusal``
into ``fill_refusal`` tomorrow. That is **R162** exactly: an assertion
that cannot come out false.

``/P 2`` is the value that separates them. Per ISO 32000-1 §12.8.4
Table 254, a DocMDP transform with ``/P 2`` permits **filling in forms
and signing**, while still forbidding other changes to the document —
which is what a *certified fillable form*, the ordinary real-world case,
looks like.

WHAT IS IN THE FILE
-------------------
``demo-form.pdf``'s structure — a text field ``FullName`` and a check box
``Subscribe``, both widget-merged into their field dictionaries — plus:

* ``/Perms << /DocMDP 8 0 R >>`` on the catalog (§12.8.2.2), which is
  what makes ``census()`` report ``perms_enforced = true``;
* object 8, a signature dictionary (``/Type /Sig`` + ``/ByteRange``
  make it one per Table 252) carrying a ``/DocMDP`` transform whose
  ``/TransformParams`` has ``/P 2``.

The ``/ByteRange`` and ``/Reference`` entries are structural
placeholders — there are no real signed bytes, and there deliberately
are none. The guards under test read census-visible *structure*; they do
not verify a signature, and a fixture carrying real cryptography would
test a claim pdfcer does not make while adding a key nobody can rotate.

Two fields rather than one, on purpose: deletion of a *text* field and of
a *button* travel slightly different paths in the panel, and a one-field
fixture would let a regression in either hide.

PROVENANCE
----------
100% byte-authored here — no PDF library is involved, so the fixture
cannot inherit a bug from the code it is used to test (project rule 7 /
``LEGAL.md`` §5: synthetic or rights-cleared only, never a downloaded
real-world file).

USAGE
-----
    python tools/gen-form-certified-p2-fixture.py
"""

from pathlib import Path

OUT = Path("fixtures/synthetic/forms/certified-p2-form.pdf")


def stream_obj(dict_prefix: bytes, content: bytes) -> bytes:
    """A stream object: dict + /Length + the stream body."""
    return (
        dict_prefix
        + b" /Length %d >>\nstream\n" % len(content)
        + content
        + b"\nendstream"
    )


def build() -> bytes:
    objs: dict[int, bytes] = {}
    # The catalog carries BOTH the AcroForm and the /Perms /DocMDP entry.
    # /Perms is what census() reads to decide `perms_enforced`; without it
    # a signature dictionary alone does not constrain anything.
    objs[1] = (
        b"<< /Type /Catalog /Pages 2 0 R /Perms << /DocMDP 8 0 R >> "
        b"/AcroForm << /Fields [4 0 R 5 0 R] /DA (/Helv 0 Tf 0 g) "
        b"/DR << /Font << /Helv << /Type /Font /Subtype /Type1 "
        b"/BaseFont /Helvetica >> >> >> >> >>"
    )
    objs[2] = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"
    objs[3] = (
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] "
        b"/Resources << >> /Annots [4 0 R 5 0 R] >>"
    )
    # A merged field+widget (Shape A) — the common case, and the one whose
    # single-widget deletion the GUI must present as "Delete field".
    objs[4] = (
        b"<< /FT /Tx /T (FullName) /TU (Full name) /Subtype /Widget "
        b"/Rect [20 150 250 172] /P 3 0 R /MK << /BC [0 0 0] >> >>"
    )
    objs[5] = (
        b"<< /FT /Btn /T (Subscribe) /V /Off /AS /Off /Subtype /Widget "
        b"/Rect [20 100 34 114] /P 3 0 R "
        b"/AP << /N << /Yes 6 0 R /Off 7 0 R >> >> >>"
    )
    objs[6] = stream_obj(
        b"<< /Type /XObject /Subtype /Form /BBox [0 0 14 14]",
        b"0 0 14 14 re f 4 4 6 6 re f",
    )
    objs[7] = stream_obj(
        b"<< /Type /XObject /Subtype /Form /BBox [0 0 14 14]",
        b"0 0 14 14 re S",
    )
    # THE POINT OF THE FIXTURE. /P 2 = "filling in forms and signing is
    # permitted" (§12.8.4 Table 254). /P 1 would refuse both operations
    # and prove nothing about the gate distinction.
    objs[8] = (
        b"<< /Type /Sig /Filter /Adobe.PPKLite /ByteRange [0 1 2 3] "
        b"/Reference [ << /TransformMethod /DocMDP "
        b"/TransformParams << /P 2 >> >> ] >>"
    )

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


def main() -> int:
    OUT.parent.mkdir(parents=True, exist_ok=True)
    data = build()
    OUT.write_bytes(data)
    print(f"wrote {OUT} ({len(data)} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
