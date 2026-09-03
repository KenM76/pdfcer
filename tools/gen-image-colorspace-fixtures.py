#!/usr/bin/env python3
"""Generate single-image PDFs that isolate ONE image colour space each.

WHY THIS EXISTS
===============
`pdfcer-render`'s image decoder converts a sample tuple to sRGB through a
different code path for every colour-space family, and a whole-page raster
diff cannot attribute a divergence to a colour space when the page also
carries text, vectors, or a second image. These fixtures make the
attribution trivial by construction: ONE image, filling the page, nothing
else. Every pixel of the resulting raster is a measurement of that colour
space and of nothing else.

They were written for `Pass 85.x` (image colour spaces), where eighteen
images in the operator's print-conformance suite file failed to paint at all
because five spaces were an outright refusal in the image path.

WHY A GENERATOR RATHER THAN CHECKED-IN PDFs
===========================================
Project rule 7: fixture PDFs are synthetic or rights-cleared only. A
generator is strictly better than a checked-in synthetic binary — it states
in source form exactly what the file contains, so a reader can see that the
`/Decode` default under test is the spec's and not a copy of whatever some
producer emitted. It also keeps ~a dozen binaries out of a public repo's
history. Same convention as `tools/gen-embed-fixtures.py` and
`tools/gen-unembed-fixtures.py`.

Output goes to a directory of the caller's choosing — by convention
`D:\\Dev\\temp\\img-fixtures`, OUTSIDE the repository, because the repo is
public and generated test binaries have no business in its history.

USAGE
=====
    python tools/gen-image-colorspace-fixtures.py <output-dir>

Then measure with either of:

    python tools/render-parity/render_parity.py <output-dir> \\
        --pages-per-file 0 --dpi 150 --out <diff-dir> --band 0.0294
    python tools/check-image-colorspace-truth.py <output-dir>

The second is the stronger oracle for the CIE-based spaces and should be
preferred where it applies — see its module docstring.

WHAT EACH FIXTURE EXERCISES
===========================
  separation   1 component -> DeviceCMYK via a type-2 exponential tint
               transform. The simplest tint path.
  devicen-2    2 components (a duotone) -> DeviceCMYK via a type-4
               PostScript calculator transform. Exercises the multi-input
               transform AND the tint-cache key packing, and the calculator
               is the expensive path the cache exists for.
  lab          3 components, L in 0..100 with a/b spanning NEGATIVE to
               positive. This is the case where clamping every component to
               0..1 -- the behaviour before Pass 85.x for every non-device
               space -- flattens the image toward black while still
               producing a plausible-looking picture, which is the worst
               kind of wrong.
  calgray      1 component through /Gamma and a white point.
  calrgb       3 components through a per-component /Gamma and a 3x3
               /Matrix. Non-identity matrix on purpose: an identity matrix
               would pass even if the matrix were ignored entirely.
  sep-all      /Separation /All: paints every colorant at once (8.6.6.4).
  sep-none     /Separation /None: shall paint NOTHING (8.6.6.4). Not white
               -- transparent. An opaque white image looks identical on a
               blank page and ERASES any backdrop underneath it.

SPEC NOTES THAT THE FIXTURES DEPEND ON
======================================
ISO 32000-1 Table 89 (`/Decode` defaults for images): for a Lab image the
default Decode is `[0 100 amin amax bmin bmax]`, where the a/b bounds come
from the colour space's own `/Range` -- NOT `[0 1 0 1 0 1]`. With
`/Range [-100 100 -100 100]` an 8-bit sample therefore decodes as
`L = v/255*100`, `a = -100 + v/255*200`, `b = -100 + v/255*200`.

ISO 32000-1 7.3.8.1: a stream shall be an INDIRECT object. An earlier
revision of this generator inlined the DeviceN tint-transform stream inside
the colour-space array, which is invalid; pdfcer correctly refused the file
and the fixture silently dropped out of every measurement as a "skip".
Function streams are emitted as numbered objects here for that reason.
"""

import os
import sys
import zlib

W = H = 64  # image pixels
PW = PH = 128  # page points
D65 = "[0.9505 1.0 1.089]"


def build(objects: list[tuple[int, bytes]]) -> bytes:
    """Assemble numbered objects into a minimal, classic-xref PDF 1.7 file.

    Deliberately hand-rolled rather than written through `pdfcer`: a
    fixture whose bytes were produced by the program under test cannot
    falsify that program. The xref offsets are computed from the real
    buffer positions so the file loads under a strict parser.
    """
    buf = bytearray(b"%PDF-1.7\n")
    offs: dict[int, int] = {}
    for num, body in objects:
        offs[num] = len(buf)
        buf += f"{num} 0 obj\n".encode() + body + b"\nendobj\n"
    xref_at = len(buf)
    n = len(objects) + 1
    buf += f"xref\n0 {n}\n0000000000 65535 f \n".encode()
    for num in range(1, n):
        buf += f"{offs[num]:010d} 00000 n \n".encode()
    buf += f"trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n".encode()
    return bytes(buf)


def make(outdir, name, cs, ncomp, sample, extra=()):
    """Write one single-image fixture.

    `cs` is the raw colour-space bytes for the image dictionary; `extra` is
    a list of additional numbered objects (function streams, which must be
    indirect per 7.3.8.1). `sample(x, y)` returns `ncomp` ints in 0..255.
    """
    data = bytearray()
    for y in range(H):
        for x in range(W):
            data += bytes(sample(x, y))
    comp = zlib.compress(bytes(data))
    content = f"q {PW} 0 0 {PH} 0 0 cm /Im Do Q".encode()
    objs = [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            f"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 {PW} {PH}] >>".encode(),
        ),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources "
            b"<< /XObject << /Im 5 0 R >> >> >>",
        ),
        (4, b"<< /Length %d >>\nstream\n" % len(content) + content + b"\nendstream"),
        (
            5,
            b"<< /Type /XObject /Subtype /Image /Width %d /Height %d "
            b"/BitsPerComponent 8 /ColorSpace " % (W, H)
            + cs
            + b" /Filter /FlateDecode /Length %d >>\nstream\n" % len(comp)
            + comp
            + b"\nendstream",
        ),
    ]
    objs.extend(extra)
    path = os.path.join(outdir, name + ".pdf")
    with open(path, "wb") as fh:
        fh.write(build(objs))
    print("wrote", path)


def main(outdir: str) -> None:
    os.makedirs(outdir, exist_ok=True)

    # Type-2 exponential tint transform, 1-in 4-out. Inline in the array is
    # legal for a DICTIONARY (only streams must be indirect).
    fn1 = (
        b"<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] "
        b"/C1 [0.9 0.2 0.1 0.05] /N 1 >>"
    )

    make(outdir, "separation",
         b"[/Separation /SuiteSpot /DeviceCMYK " + fn1 + b"]",
         1, lambda x, y: (x * 4,))

    # Type-4 calculator, 2-in 4-out, as an INDIRECT stream (7.3.8.1).
    ps = b"{ exch dup 3 1 roll add 0.5 mul 0 0 }"
    fn4 = (
        b"<< /FunctionType 4 /Domain [0 1 0 1] /Range [0 1 0 1 0 1 0 1] "
        b"/Length %d >>\nstream\n" % len(ps) + ps + b"\nendstream"
    )
    make(outdir, "devicen-2",
         b"[/DeviceN [/InkA /InkB] /DeviceCMYK 6 0 R]",
         2, lambda x, y: (x * 4, y * 4), extra=[(6, fn4)])

    # L across x, a across y, b across the diagonal, so all THREE components
    # vary and none is a constant a decoder could ignore.
    #
    # /Range is deliberately ASYMMETRIC -- [-100 100] for a but [-60 60] for
    # b. The obvious choice, a symmetric [-100 100 -100 100], is BLIND to the
    # single most likely defect in this decode: transposing the a and b range
    # pairs. With symmetric bounds amin==bmin and amax==bmax, so the
    # transposition is arithmetically a no-op and every fixture still passes.
    # That was not hypothetical -- it was found by sabotaging
    # `component_range` to swap the pairs and watching
    # `tools/check-image-colorspace-truth.py` stay GREEN. Asymmetric bounds
    # make the same sabotage fail loudly.
    make(outdir, "lab",
         b"[/Lab << /WhitePoint " + D65.encode()
         + b" /Range [-100 100 -60 60] >>]",
         3, lambda x, y: (x * 4, y * 4, (x + y) * 2))

    make(outdir, "calgray",
         b"[/CalGray << /WhitePoint " + D65.encode() + b" /Gamma 2.2 >>]",
         1, lambda x, y: (x * 4,))

    # Non-identity /Matrix: the sRGB primaries under D65. An identity
    # matrix would let a decoder that ignores /Matrix entirely still pass.
    srgb_matrix = (b"[0.4124 0.2126 0.0193 0.3576 0.7152 0.1192 "
                   b"0.1805 0.0722 0.9505]")
    make(outdir, "calrgb",
         b"[/CalRGB << /WhitePoint " + D65.encode()
         + b" /Gamma [2.2 2.2 2.2] /Matrix " + srgb_matrix + b" >>]",
         3, lambda x, y: (x * 4, y * 4, 128))

    make(outdir, "sep-all",
         b"[/Separation /All /DeviceCMYK " + fn1 + b"]",
         1, lambda x, y: (x * 4,))

    make(outdir, "sep-none",
         b"[/Separation /None /DeviceCMYK " + fn1 + b"]",
         1, lambda x, y: (x * 4,))


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__.strip().splitlines()[0] + "\n\nusage: "
                 "gen-image-colorspace-fixtures.py <output-dir>")
    main(sys.argv[1])
