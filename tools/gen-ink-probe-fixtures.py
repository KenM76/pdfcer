#!/usr/bin/env python3
"""gen-ink-probe-fixtures — the oracle for the INK PROBE, and for one claim
about the colorant compositor that had never been tested directly.

WHAT THIS BUILDS
================
Two pages carrying the **same single opaque CMYK fill**, differing in exactly
one thing: whether the page declares a subtractive blending colour space.

| file | page group | what pdfcer does with it |
|---|---|---|
| `flat-cmyk-subtractive.pdf` | `/Group << /S /Transparency /CS /DeviceCMYK >>` | composites in the four-colorant buffer |
| `flat-cmyk-additive.pdf`    | none                                             | composites on screen, in sRGB |

Everything else — media box, geometry, operands, object numbering — is
byte-identical between the two, so a difference in any measurement is
attributable to the blending space and to nothing else.

THE CLAIM UNDER TEST, AND WHY IT NEEDED A FIXTURE OF ITS OWN
============================================================
**For a single opaque paint over an empty page, a correct colorant composite
is the IDENTITY on its operand.** Nothing is blended into it: the backdrop is
transparent, the alpha is 1, the blend mode is Normal. So the four numbers
sitting in the buffer at the moment of the exit conversion to sRGB must be the
four numbers the content stream wrote.

That sounds too obvious to test, which is exactly why it had never been. It is
the hinge of an attribution question raised by the sibling `iccce` project on
2026-08-29: a colorant buffer does two separable things — it **composites** in
ink (pdfcer's, under decision 064) and it **converts** the result to sRGB on the
way out (iccce's, under the same decision) — and pdfcer's only instrument for
turning the buffer off, `--max-cmyk-buffer-bytes`, turns off *both at once*.
No measurement taken through that switch can say which half moved a pixel.

The probe reads the buffer BETWEEN the two stages, which splits them. These
fixtures are what says the reading is trustworthy:

  * operand in, same operand out  ⇒ the composite is innocent, and any
    remaining error is in the conversion;
  * operand in, different operand out ⇒ the composite moved it, and the
    conversion is not the whole story.

THE OPERANDS, AND WHY THESE
===========================
`0.75 0 1 0 k` — a saturated green. It is the operand of the one patch this
question was raised about, and it is chosen over a neutral for the reason a
neutral would be useless: a colour near the achromatic axis moves barely at
all through a bad round trip, so a fixture built on one passes whether or not
the defect is present (`R225` — a fixture whose two candidate answers coincide
cannot tell them apart).

★ **Three of the four components are exactly representable and one is not.**
0.75, 0.0 and 1.0 survive any plausible intermediate precision unchanged; if a
future change routes the operand through 8-bit storage, `0.75` stays `0.75`
and the test would not notice. That is deliberate — the probe's contract is
about the compositor, not about precision — but a reader should not mistake
"these numbers came back exactly" for "no quantisation happened anywhere".

★★ **The second page is a genuine control, not decoration.** A probe
implementation that reported the *content stream's* operands rather than the
*buffer's* contents would pass every assertion on page one. It cannot pass on
page two, where there is no buffer at all and the honest answer is "there are
no colorant values" — which is a different report, not a smaller one.

USAGE
=====
    python tools/gen-ink-probe-fixtures.py

Writes into `fixtures/synthetic/ink-probe/`. Rights-cleared by construction:
every byte is generated here (`LEGAL.md` §5).
"""

from __future__ import annotations

import pathlib

OUT = pathlib.Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "ink-probe"

# The operand under test. See THE OPERANDS above for why these four numbers.
INK = (0.75, 0.0, 1.0, 0.0)

# A 200x200 pt page with a 100x100 pt patch at (50, 50). Big enough that a
# probe aimed at the middle cannot land on an antialiased edge at any scale a
# test would plausibly use, which is the failure mode
# `feedback_a_crop_rectangle_is_a_measurement_instrument` is about.
PAGE = 200
PATCH_LL = 50
PATCH_SIDE = 100


def build(objs: dict[int, bytes]) -> bytes:
    """Serialise a numbered object map as a classic-xref PDF.

    A plain cross-reference TABLE rather than a stream, for the same reason
    `gen-devicen-image-fixtures.py` gives: these fixtures test colour, and
    anyone debugging one should be able to read the bytes without inflating
    anything first.
    """
    assert sorted(objs) == list(range(1, len(objs) + 1)), (
        f"object numbers must be contiguous from 1; got {sorted(objs)}"
    )
    out = bytearray(b"%PDF-1.7\n")
    offsets: dict[int, int] = {}
    for n in sorted(objs):
        offsets[n] = len(out)
        out += b"%d 0 obj\n" % n + objs[n] + b"\nendobj\n"
    xref = len(out)
    out += b"xref\n0 %d\n" % (len(objs) + 1)
    out += b"0000000000 65535 f \n"
    for n in sorted(objs):
        out += b"%010d 00000 n \n" % offsets[n]
    out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (
        len(objs) + 1,
        xref,
    )
    return bytes(out)


def stream(dict_body: bytes, payload: bytes) -> bytes:
    """A stream object whose `/Length` is computed from its own payload.

    Typed lengths are the classic way to produce a fixture that half-works.
    """
    return dict_body[:-2] + b" /Length %d >>\nstream\n" % len(payload) + payload + b"\nendstream"


def page(*, subtractive: bool) -> bytes:
    """One page: a single opaque `k` fill, with or without a CMYK page group.

    Nothing else is on the page. Every additional object would be another
    thing that could move the buffer's contents, and the claim under test is
    precisely that *nothing* moves them.
    """
    group = b"/Group << /S /Transparency /CS /DeviceCMYK >> " if subtractive else b""
    objs: dict[int, bytes] = {}
    objs[1] = b"<< /Type /Catalog /Pages 2 0 R >>"
    objs[2] = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"
    objs[3] = (
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 %d %d] " % (PAGE, PAGE)
        + group
        + b"/Resources << >> /Contents 4 0 R >>"
    )
    operands = b" ".join(b"%.10g" % v for v in INK)
    content = b"q %s k %d %d %d %d re f Q" % (
        operands,
        PATCH_LL,
        PATCH_LL,
        PATCH_SIDE,
        PATCH_SIDE,
    )
    objs[4] = stream(b"<< >>", content)
    return build(objs)


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    for name, subtractive in (
        ("flat-cmyk-subtractive.pdf", True),
        ("flat-cmyk-additive.pdf", False),
    ):
        (OUT / name).write_bytes(page(subtractive=subtractive))
        print(f"wrote {OUT / name}")


if __name__ == "__main__":
    main()
