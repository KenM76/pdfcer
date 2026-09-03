"""Generate `fixtures/synthetic/signature/*.pdf`.

WHY THESE FIXTURES EXIST
========================

`signature::byte_range_coverage` measures what a signature's `/ByteRange`
protects, against the file's real length. Nothing in the existing corpus
exercises the shape it reads:

  * `forms/certified-p2-form.pdf` carries `/ByteRange [0 1 2 3]` — a stub
    — and **no signature field at all**; its signature dictionary hangs
    off `/Perms /DocMDP`. Good for the certification census, useless here.
  * `forms/unfillable-fields-form.pdf` has a real `/FT /Sig` field with
    **no `/V`** — an unsigned signature field, which is a form waiting to
    be signed, not a signature.

So the one shape that matters — a `/FT /Sig` field whose `/V` is a
signature dictionary with a real `/ByteRange` — was untested, and a test
against either existing fixture would have been vacuous in a way that
looks like a pass.

WHAT THIS BUILDS (ISO 32000-1 §12.8.1, Table 252)
=================================================

Three one-page documents, each with a single `/FT /Sig` field:

  1. `signed-full-coverage.pdf` — `/ByteRange` reaching the LAST BYTE of
     the file, in the canonical two-pair shape straddling `/Contents`.
     The good case, and the one whose offsets have to be computed after
     the file is laid out rather than guessed.

  2. `signed-short-coverage.pdf` — identical, except the second pair
     stops short, leaving a tail the signature does not cover.
     **This is conforming**: §12.8.1 makes whole-file coverage a `should`
     ("Other ranges may be used but ... their use is not recommended"),
     so the fixture proves pdfcer reports it without calling it malformed.

  3. `signed-malformed-range.pdf` — ranges that OVERLAP, which Table 252's
     "exact byte range" does not permit. Distinct from case 2: this one
     really is wrong, and the two must not report the same way.

THE `/Contents` HOLE IS REAL, NOT DECORATIVE
============================================

Each signature dictionary carries a `/Contents` hex string of fixed
width, and the two `/ByteRange` pairs are computed to straddle it exactly
— the layout a real signer produces, where the placeholder is written
first and the digest is computed over everything except that hole
(§12.8.3.3). Writing the pairs as round numbers would let an off-by-one
in the straddle arithmetic pass unnoticed.

No cryptography is involved and none is claimed: `/Contents` is filler.
These fixtures are for the COVERAGE measurement, which is arithmetic over
byte offsets and never inspects the signature value.

Run from the repository root:  `python tools/gen-signature-fixtures.py`
"""

import pathlib

OUT = pathlib.Path("fixtures/synthetic/signature")

#: Width of the `/Contents` placeholder, in hex digits. A real signer
#: reserves a fixed span before knowing the signature's length; the exact
#: number is arbitrary here, but it must be even (hex pairs) and it must
#: be a CONSTANT, because the byte-range arithmetic below is computed
#: from the laid-out file rather than from this value.
CONTENTS_HEX_DIGITS = 512


def build(name, coverage):
    """Assemble one fixture.

    `coverage` selects the `/ByteRange` shape:
      "full"      — the second pair reaches the last byte
      "short"     — the second pair stops 200 bytes early
      "malformed" — the second pair starts before the first one ends

    The offsets cannot be known until the file is laid out, because they
    are positions in the finished bytes. So the file is built ONCE with a
    placeholder byte range of the right width, the real offsets are
    measured from that layout, and the placeholder is overwritten in
    place — which is exactly how a real signer works, and the reason the
    placeholder is padded to a fixed width.
    """
    contents = b"0" * CONTENTS_HEX_DIGITS
    # A fixed-width placeholder: ten digits per number is enough for any
    # file this script produces, and keeping the width constant means
    # overwriting it cannot change any offset.
    br_placeholder = b"[0000000000 0000000000 0000000000 0000000000]"

    objs = {}
    objs[1] = (
        b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [10 0 R] "
        b"/SigFlags 3 >> >>"
    )
    objs[2] = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"
    objs[3] = (
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] "
        b"/Resources << >> /Annots [11 0 R] >>"
    )
    # The signature FIELD, merged with its widget. `/V` points at the
    # signature dictionary — the shape `byte_range_coverage` reads.
    objs[10] = b"<< /FT /Sig /T (Approval) /V 12 0 R /Kids [11 0 R] >>"
    objs[11] = (
        b"<< /Parent 10 0 R /Subtype /Widget /Rect [20 20 200 60] "
        b"/P 3 0 R /F 132 >>"
    )
    objs[12] = (
        b"<< /Type /Sig /Filter /Adobe.PPKLite /SubFilter /adbe.pkcs7.detached "
        b"/Name (A. Signer) /M (D:20260810120000Z) "
        b"/ByteRange " + br_placeholder + b" /Contents <" + contents + b"> >>"
    )

    buf = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    off = {}
    for n in sorted(objs):
        off[n] = len(buf)
        buf += b"%d 0 obj\n" % n + objs[n] + b"\nendobj\n"

    xref_at = len(buf)
    size = max(objs) + 1
    buf += b"xref\n0 %d\n0000000000 65535 f \n" % size
    for n in range(1, size):
        if n in off:
            buf += b"%010d 00000 n \n" % off[n]
        else:
            buf += b"0000000000 65535 f \n"
    buf += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (
        size,
        xref_at,
    )

    # Now the file is laid out, so the hole's real position is known.
    # The hole is the WHOLE /Contents token INCLUDING its `<` and `>`
    # delimiters (ISO 32000-1 §12.8.3.3.2, spec RAG `SI-W3`: "a = offset
    # of <, b = offset just past >"). An earlier version of this script
    # excluded only the hex digits, which `signature::verify` (Pass 10.2)
    # correctly refuses as "not the /Contents hex string" — the coverage
    # arithmetic never noticed, because it counts bytes, not delimiters.
    lt = buf.index(b"<" + contents)
    gt = lt + 1 + len(contents) + 1  # just past `>`
    total = len(buf)

    first = (0, lt)
    if coverage == "full":
        second = (gt, total - gt)
    elif coverage == "short":
        # Conforming but under-protecting: 200 bytes past the end of the
        # covered range exist in the file.
        second = (gt, max(0, total - gt - 200))
    elif coverage == "malformed":
        # Starts BEFORE the first pair ends — Table 252's "exact" range
        # does not permit overlap.
        second = (max(0, first[1] - 50), total - gt)
    else:
        raise ValueError(coverage)

    real = b"[%d %d %d %d]" % (first[0], first[1], second[0], second[1])
    # Pad to the placeholder's exact width so no offset shifts. If it does
    # not fit, the arithmetic above is wrong and silence would hide it.
    if len(real) > len(br_placeholder):
        raise AssertionError(f"byte range {real!r} exceeds the reserved width")
    real = real[:-1] + b" " * (len(br_placeholder) - len(real)) + b"]"
    i = buf.index(br_placeholder)
    buf[i : i + len(br_placeholder)] = real

    OUT.mkdir(parents=True, exist_ok=True)
    path = OUT / name
    path.write_bytes(bytes(buf))
    print(f"wrote {path} {len(buf)} bytes  ByteRange={real.decode().strip()}")


build("signed-full-coverage.pdf", "full")
build("signed-short-coverage.pdf", "short")
build("signed-malformed-range.pdf", "malformed")
