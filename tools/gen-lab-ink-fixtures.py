#!/usr/bin/env python3
"""gen-lab-ink-fixtures — the oracle for `Pass 242.0`.

WHAT THIS BUILDS, AND WHY IT NEEDS NO REFERENCE RENDER
======================================================
Each fixture draws **the same `Lab` colour three ways on one page**: as a flat
path fill, as a direct 8-bit sampled image, and as an `/Indexed` image whose
one palette entry is that colour. A correct renderer paints all three the
same colour AND deposits the same ink for all three, so the assertions are
*fill == image == palette* at the pixel and at the ink probe, and need no
external reference.

`tools/gen-icc-rgb-fixtures.py`'s oracle, applied to a space that has
colorimetry but no profile and no colorants.

THE DEFECT IT WAS BUILT FOR
===========================
On a page that composites in ink, a `Lab` (or `CalRGB`, `CalGray`) colour
used to reach the colorant buffer by the worst route available: to sRGB
through the CIE decode, then back to four inks through the max-GCR
`rgb_to_cmyk` round trip. A colorimetric grey was separated by a formula that
knows nothing about the press. Measured on a print-conformance patch: a
`Lab (60, 0, 0)` backdrop separated to `K = 0.43` alone, where the document's
own output intent separates it to roughly `(0.38, 0.31, 0.31, 0.18)`; a
`ColorBurn` over the K-only version burned to solid black, and the patch's
trap X — authored to vanish under the press separation — stood out.

`Pass 242.0` gives a CIE colour the PCS route: its XYZ, adapted to D50, goes
straight into the output intent's B2A table. Fills through
`Interpreter::authored_cmyk`, images through `image::Space::Special { pcs }`.

THE TWO PAGES
=============
| file | page group | `/OutputIntents` | what the third column proves |
|---|---|---|---|
| `lab-three-ways-subtractive.pdf` | `/CS /DeviceCMYK` | `../icc-rgb/dest-cmyk.icc` | the PCS route: all three separate through the output intent |
| `lab-three-ways-subtractive-no-intent.pdf` | `/CS /DeviceCMYK` | none | the control: no PCS route exists, all three bridge through `rgb_to_cmyk` from the same sRGB |

The two files differ ONLY in the `/OutputIntents` entry, so a difference in
the probed ink between them is attributable to the PCS route and nothing else
— and if the route never ran, the two would be byte-identical at the probe.

★★ The colour is `Lab (60, 0, 0)` — the patch's own grey, and a NEUTRAL on
purpose. A press profile separates a neutral into all four inks (grey
component replacement), where `rgb_to_cmyk` separates it into K alone. The
two candidate answers are therefore far apart in C, M and Y, which is what
lets the test tell them apart (rule R225).

★ Samples are exactly representable: `L* = 60` is byte 153 with the default
`[0 100]` decode (153 × 100 / 255 = 60.0), and `a* = b* = 0` is byte 128
under the space's `/Range [-128 127]` (−128 + 128 × 255/255 = 0.0), so the
fill's operands and the image's samples are the same numbers.

USAGE
=====
    python tools/gen-lab-ink-fixtures.py

Writes into `fixtures/synthetic/lab-ink/`. Rights-cleared by construction:
every byte is generated here except the embedded CMYK profile, which is the
author's own MIT-licensed work from a sibling project (see
`fixtures/synthetic/icc-rgb/PROVENANCE.md`).
"""

from __future__ import annotations

import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent / "fixtures" / "synthetic"
OUT = ROOT / "lab-ink"
DEST = ROOT / "icc-rgb" / "dest-cmyk.icc"

# Lab (60, 0, 0): operands for the fill, bytes for the images.
OPERANDS = (60.0, 0.0, 0.0)
SAMPLES = (153, 128, 128)


def build(objs: dict[int, bytes]) -> bytes:
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
    return dict_body[:-2] + b" /Length %d >>\nstream\n" % len(payload) + payload + b"\nendstream"


def page(objs: dict[int, bytes], *, intent: bytes) -> None:
    """Three 60x60 pt boxes at x = 10, 80, 150 on a 230x100 page.

    Box 1 is the fill, 2 the direct image, 3 the `/Indexed` image.
    """
    objs[1] = b"<< /Type /Catalog /Pages 2 0 R " + intent + b">>"
    objs[2] = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"
    objs[3] = (
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 230 100] "
        b"/Group << /S /Transparency /CS /DeviceCMYK >> "
        b"/Resources << /ColorSpace << /Cs0 5 0 R /Cs1 6 0 R >> "
        b"/XObject << /Im0 7 0 R /Im1 8 0 R >> >> /Contents 4 0 R >>"
    )
    operands = b" ".join(b"%.10g" % v for v in OPERANDS)
    content = (
        b"q /Cs0 cs " + operands + b" scn 10 20 60 60 re f Q\n"
        b"q 60 0 0 60 80 20 cm /Im0 Do Q\n"
        b"q 60 0 0 60 150 20 cm /Im1 Do Q"
    )
    objs[4] = stream(b"<< >>", content)


def write(name: str, data: bytes) -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / name).write_bytes(data)
    print(f"  {name}  {len(data)} bytes")


def main() -> int:
    print(f"writing to {OUT}")
    dest = DEST.read_bytes()
    assert dest[16:20] == b"CMYK", "dest-cmyk.icc is not a CMYK profile"

    for name, with_intent in [
        ("lab-three-ways-subtractive.pdf", True),
        ("lab-three-ways-subtractive-no-intent.pdf", False),
    ]:
        objs: dict[int, bytes] = {}
        intent = b"/OutputIntents [9 0 R] " if with_intent else b""
        page(objs, intent=intent)
        objs[5] = (
            b"[/Lab << /WhitePoint [0.9642 1.0 0.8249] "
            b"/Range [-128 127 -128 127] >>]"
        )
        objs[6] = b"[/Indexed 5 0 R 0 <%02X%02X%02X>]" % SAMPLES
        objs[7] = stream(
            b"<< /Type /XObject /Subtype /Image /Width 8 /Height 8 "
            b"/BitsPerComponent 8 /ColorSpace 5 0 R >>",
            bytes(SAMPLES) * 64,
        )
        objs[8] = stream(
            b"<< /Type /XObject /Subtype /Image /Width 8 /Height 8 "
            b"/BitsPerComponent 8 /ColorSpace 6 0 R >>",
            bytes([0]) * 64,
        )
        if with_intent:
            objs[9] = (
                b"<< /Type /OutputIntent /S /GTS_PDFX "
                b"/OutputConditionIdentifier (pdfcer synthetic CMYK) "
                b"/DestOutputProfile 10 0 R >>"
            )
            objs[10] = stream(b"<< /N 4 >>", dest)
        write(name, build(objs))
    return 0


if __name__ == "__main__":
    sys.exit(main())
