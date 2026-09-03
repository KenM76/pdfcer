#!/usr/bin/env python3
"""gen-devicen-image-fixtures — the oracle for `Pass 140.0`.

WHAT THIS BUILDS, AND WHY IT NEEDS NO REFERENCE RENDER
======================================================
Each fixture draws **the same authored colour twice on one page**: once as a
flat path fill, once as a sampled image whose every texel carries the identical
operands. Same colour space, same tint values, same page. A correct renderer
paints them the same colour, so the assertion is *fill == image* and needs no
external reference, no geometry, and nothing remembered between runs.

This is `tools/gen-shading-ink-fixtures.py`'s oracle applied one object type
over: that file pairs a fill against a *shading*, this one pairs it against an
*image*.

THE DEFECT IT WAS BUILT FOR
===========================
On a page whose group colour space is `/DeviceCMYK`, pdfcer composites in a
four-colorant buffer. A `Separation`/`DeviceN` image used to convert through
its tint transform **to sRGB** and never to its `DeviceCMYK` alternate, so it
entered that buffer through a `CMYK -> sRGB -> CMYK` round trip whose return
leg is a naive formula rather than the calibrated conversion that went out.
The measured symptom was a five-colorant `DeviceN` photograph rendering
visibly desaturated — washed toward grey where the reference was saturated
green.

`Pass 130.1` fixed the same defect for `DeviceCMYK` images; `Pass 140.0` fixes
it for `Separation`/`DeviceN` ones, both direct and behind an `/Indexed`
palette.

THE FOUR PAGES, AND WHY EACH EXISTS
===================================
| file | space | route it exercises |
|---|---|---|
| `separation-image-vs-fill-cmyk.pdf` | `/Separation /SpotGreen /DeviceCMYK` | direct, spot-only — the shape whose overprint planes answer `None` |
| `devicen-image-vs-fill-cmyk.pdf` | `/DeviceN [/Cyan /Magenta] /DeviceCMYK` | direct, process-named — the shape whose authored tints and transform output DIFFER |
| `duotone-image-vs-fill-cmyk.pdf` | `/Indexed` over the `/Separation` above | the palette route, resolved at table-build time |
| `separation-image-vs-fill-rgb.pdf` | the same `/Separation`, no page group | the ADDITIVE control — no colorant buffer, so no round trip existed to fix |

★ The `/DeviceN` page is deliberately the one whose **tint transform is not a
pass-through**: it maps `(t0, t1)` to `(0.9*t0, 0.8*t1, 0.1, 0.05)`, so the
components the source *specified* (Table 149's question, `[t0, t1, 0, 0]`) and
the components the transform *produces* are different numbers. A fixture whose
transform is the identity cannot tell those two questions apart, and telling
them apart is the whole content of this Pass's named trap.

★★ Tint values are chosen to be EXACTLY representable at 8 bits per component
— 0.4 is 102/255 and 0.8 is 204/255 — so the fill's operand and the image's
sample are the same number rather than nearly the same. Without that, every
assertion would carry a quantisation slop of up to 1/510 that has nothing to do
with the defect and would mask a small real one.

USAGE
=====
    python tools/gen-devicen-image-fixtures.py

Writes into `fixtures/synthetic/devicen-image/`. Rights-cleared by
construction: every byte is generated here (`LEGAL.md` §5).
"""

from __future__ import annotations

import pathlib
import sys

OUT = pathlib.Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "devicen-image"

# Exactly representable at 8 bpc: 102/255 and 204/255.
T1 = 0.4
T2 = 0.8
S1 = 102
S2 = 204


def build(objs: dict[int, bytes]) -> bytes:
    """Serialise a numbered object map as a classic-xref PDF.

    Deliberately a plain cross-reference TABLE rather than a stream: these
    fixtures test colour, and a reader debugging one should be able to read
    its bytes without decompressing anything.
    """
    # ★ A classic xref table is written as ONE contiguous subsection, so a gap
    # in the numbering silently shifts every entry after it and the file dies
    # with "object at xref offset N declares X, xref expected Y". Caught here
    # rather than by a renderer, because the message names the symptom and not
    # the cause. (It happened while writing this file: the duotone page
    # allocated 8 and 9 for its inner space while 7 was never used.)
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
    """A stream object with its `/Length` filled in from the payload itself.

    A hand-written `/Length` that disagrees with the bytes is the classic way
    to produce a fixture that half-works, so it is computed here and never
    typed.
    """
    return dict_body[:-2] + b" /Length %d >>\nstream\n" % len(payload) + payload + b"\nendstream"


def page(objs: dict[int, bytes], *, subtractive: bool, cs_ref: bytes, ncomp: int) -> None:
    """The shared page: a flat fill on the left, an image on the right.

    Both halves are 60x120 pt boxes with a 20 pt gutter, so a test can sample
    well inside either without a boundary pixel entering the mean. The image is
    8x8 texels of one constant value, scaled up — a constant image means any
    texel is comparable to any other and the test needs no interpolation
    reasoning.
    """
    group = b"/Group << /S /Transparency /CS /DeviceCMYK >> " if subtractive else b""
    objs[1] = b"<< /Type /Catalog /Pages 2 0 R >>"
    objs[2] = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"
    objs[3] = (
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] "
        + group
        + b"/Resources << /ColorSpace << /Cs0 " + cs_ref + b" >> "
        b"/XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>"
    )
    # The fill's operands, and the image's samples, are the same numbers.
    operands = b" ".join(b"%.10g" % v for v in ([T1, T2][:ncomp] if ncomp > 1 else [T1]))
    content = (
        b"q /Cs0 cs " + operands + b" scn 20 40 60 120 re f Q\n"
        b"q 60 0 0 120 120 40 cm /Im0 Do Q"
    )
    objs[4] = stream(b"<< >>", content)


def image_object(cs_ref: bytes, samples: bytes, ncomp: int, bpc: int = 8) -> bytes:
    return stream(
        b"<< /Type /XObject /Subtype /Image /Width 8 /Height 8 "
        b"/BitsPerComponent %d /ColorSpace %s >>" % (bpc, cs_ref),
        samples,
    )


def separation_cs(objs: dict[int, bytes], first: int) -> bytes:
    """`/Separation /SpotGreen /DeviceCMYK` with a type 2 tint transform.

    `C1` is a saturated green ink rather than a neutral, because the defect
    this fixture family exists for manifests as DESATURATION — a neutral
    colorant would move barely at all through the round trip and the fixture
    would pass whether or not the fix was present.
    """
    objs[first] = b"[/Separation /SpotGreen /DeviceCMYK %d 0 R]" % (first + 1)
    objs[first + 1] = (
        b"<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] "
        b"/C1 [0.9 0.0 0.75 0.1] /N 1 >>"
    )
    return b"%d 0 R" % first


def devicen_cs(objs: dict[int, bytes], first: int) -> bytes:
    """`/DeviceN [/Cyan /Magenta] /DeviceCMYK` with a NON-pass-through transform.

    `(t0, t1) -> (0.9*t0, 0.8*t1, 0.1, 0.05)`. See this module's header for why
    the transform must not be the identity.
    """
    tint = b"{ 0.8 mul exch 0.9 mul exch 0.1 0.05 }"
    objs[first] = b"[/DeviceN [/Cyan /Magenta] /DeviceCMYK %d 0 R]" % (first + 1)
    objs[first + 1] = stream(
        b"<< /FunctionType 4 /Domain [0 1 0 1] /Range [0 1 0 1 0 1 0 1] >>", tint
    )
    return b"%d 0 R" % first


def write(name: str, data: bytes) -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / name).write_bytes(data)
    print(f"  {name}  {len(data)} bytes")


def main() -> int:
    print(f"writing to {OUT}")

    # 1. Direct /Separation, subtractive page.
    objs: dict[int, bytes] = {}
    cs = separation_cs(objs, 6)
    page(objs, subtractive=True, cs_ref=cs, ncomp=1)
    objs[5] = image_object(cs, bytes([S1]) * 64, 1)
    write("separation-image-vs-fill-cmyk.pdf", build(objs))

    # 2. The additive control: identical content, no page group.
    objs = {}
    cs = separation_cs(objs, 6)
    page(objs, subtractive=False, cs_ref=cs, ncomp=1)
    objs[5] = image_object(cs, bytes([S1]) * 64, 1)
    write("separation-image-vs-fill-rgb.pdf", build(objs))

    # 3. Direct /DeviceN with two process colorants, subtractive page.
    objs = {}
    cs = devicen_cs(objs, 6)
    page(objs, subtractive=True, cs_ref=cs, ncomp=2)
    objs[5] = image_object(cs, bytes([S1, S2]) * 64, 2)
    write("devicen-image-vs-fill-cmyk.pdf", build(objs))

    # 4. /Indexed over the /Separation — a duotone's palette route.
    #    The fill uses index 1, whose palette entry is the same tint the
    #    direct fixtures use, so all four pages paint one colour.
    objs = {}
    inner = separation_cs(objs, 7)
    objs[6] = b"[/Indexed %s 1 <00%02X>]" % (inner, S1)
    cs = b"6 0 R"
    page(objs, subtractive=True, cs_ref=cs, ncomp=1)
    # The fill's operand is an INDEX, not a tint: overwrite the content.
    objs[4] = stream(
        b"<< >>",
        b"q /Cs0 cs 1 scn 20 40 60 120 re f Q\n"
        b"q 60 0 0 120 120 40 cm /Im0 Do Q",
    )
    objs[5] = image_object(cs, bytes([1]) * 64, 1)
    write("duotone-image-vs-fill-cmyk.pdf", build(objs))

    # 5 and 6. The DISCLOSURE pair for `Pass 140.2` — an image and NOTHING
    #          else, so any colour-conversion count on the page is the
    #          image's own. A page with a fill on it cannot make this
    #          measurement, which is exactly why the defect survived: on
    #          every other fixture here the fill's conversions masked the
    #          image's absence with a plausible non-zero number.
    #
    # The broken transform is broken in TWO independent ways — declared for
    # two inputs where the space has one component, and producing three
    # outputs where `DeviceCMYK` needs four. Either alone would do; both is
    # cheap insurance against a future evaluator that tolerates one of them
    # and turns the fixture inert without failing.
    for label, fn in [
        (
            "good",
            b"<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] "
            b"/C1 [0.9 0.0 0.75 0.1] /N 1 >>",
        ),
        (
            "broken",
            b"<< /FunctionType 2 /Domain [0 1 0 1] /C0 [0 0 0] "
            b"/C1 [0.9 0.0 0.75] /N 1 >>",
        ),
    ]:
        objs = {}
        objs[1] = b"<< /Type /Catalog /Pages 2 0 R >>"
        objs[2] = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"
        objs[3] = (
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] "
            b"/Group << /S /Transparency /CS /DeviceCMYK >> "
            b"/Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>"
        )
        objs[4] = stream(b"<< >>", b"q 120 0 0 120 40 40 cm /Im0 Do Q")
        objs[6] = b"[/Separation /SpotGreen /DeviceCMYK 7 0 R]"
        objs[7] = fn
        objs[5] = image_object(b"6 0 R", bytes([S1]) * 64, 1)
        write(f"image-only-{label}-tint.pdf", build(objs))

    # 7. `Pass 238.0` — the MIXED duotone: `/Indexed` over
    #    `/DeviceN [/Black /SpotGreen]`, both entries stating black AND spot.
    #    This is the shape the print-conformance suite's duotones take, and it
    #    is the one that found TWO fill-path bugs the single-colorant duotone
    #    above could not: the fill read the palette INDEX as the tint for
    #    both its spot deposit and its authored process tints, so it landed
    #    K = 0 where the image of the same entry landed K = 0.5. The
    #    transform is Type 4 so the two operands cannot be confused by a
    #    single-input evaluator: `(k, s) -> (0.5 s, 0, s, k)`.
    objs = {}
    objs[7] = b"[/DeviceN [/Black /SpotGreen] /DeviceCMYK 8 0 R]"
    objs[8] = stream(
        b"<< /FunctionType 4 /Domain [0 1 0 1] /Range [0 1 0 1 0 1 0 1] >>",
        b"{ dup 0.5 mul 0 2 index 5 -1 roll 5 -1 roll pop }",
    )
    # Two entries: index 0 = (K 0.5, spot 0.8); index 1 = (K 0, spot 1.0).
    objs[6] = b"[/Indexed 7 0 R 1 <80CC00FF>]"
    cs = b"6 0 R"
    page(objs, subtractive=True, cs_ref=cs, ncomp=1)
    objs[4] = stream(
        b"<< >>",
        b"q /Cs0 cs 0 scn 20 40 60 120 re f Q\n"
        b"q 60 0 0 120 120 40 cm /Im0 Do Q",
    )
    objs[5] = image_object(cs, bytes([0]) * 64, 1)
    write("duotone2-image-vs-fill-cmyk.pdf", build(objs))

    # 8. `Pass 238.0` — a STENCIL MASK painted in the spot: `/ImageMask true`
    #    all-marking, filled with the `/Separation` at the same tint as the
    #    path fill beside it. §8.9.6.2 makes the stencil "a region of the
    #    page to be painted with the current colour", so the two must agree
    #    exactly as an image and a fill do. Before this Pass the stencil's
    #    texels were pre-tinted in sRGB and bridged back, and the spot never
    #    reached its plane.
    objs = {}
    cs = separation_cs(objs, 6)
    page(objs, subtractive=True, cs_ref=cs, ncomp=1)
    objs[4] = stream(
        b"<< >>",
        b"q /Cs0 cs %.10g scn 20 40 60 120 re f Q\n"
        b"q /Cs0 cs %.10g scn 60 0 0 120 120 40 cm /Im0 Do Q" % (T1, T1),
    )
    # 8x8 one-bit stencil, every sample 0 = "paint" under the default Decode.
    objs[5] = stream(
        b"<< /Type /XObject /Subtype /Image /Width 8 /Height 8 "
        b"/BitsPerComponent 1 /ImageMask true >>",
        bytes([0x00]) * 8,
    )
    write("stencil-spot-vs-fill-cmyk.pdf", build(objs))

    # 9. `Pass 238.0` — the OVERPRINT shape the suite's duotone patches test:
    #    a `1 0 1 .5 k` process mark (a check mark's ink) painted first, then
    #    the mixed duotone IMAGE over it under `/OP true`. Table 149 for a
    #    `[/Black <spot>]` source: K takes the image, C and Y stay from the
    #    mark, M stays (zero), the spot deposits. The mark therefore SURVIVES
    #    as cyan+yellow under the image's black and green. With the pre-Pass
    #    all-`Source` widening the image knocked C and Y out and the mark
    #    vanished; with no spot plane the green never landed at all.
    objs = {}
    objs[7] = b"[/DeviceN [/Black /SpotGreen] /DeviceCMYK 8 0 R]"
    objs[8] = stream(
        b"<< /FunctionType 4 /Domain [0 1 0 1] /Range [0 1 0 1 0 1 0 1] >>",
        b"{ dup 0.5 mul 0 2 index 5 -1 roll 5 -1 roll pop }",
    )
    objs[6] = b"[/Indexed 7 0 R 1 <80CC00FF>]"
    cs = b"6 0 R"
    page(objs, subtractive=True, cs_ref=cs, ncomp=1)
    objs[3] = (
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] "
        b"/Group << /S /Transparency /CS /DeviceCMYK >> "
        b"/Resources << /ColorSpace << /Cs0 " + cs + b" >> "
        b"/ExtGState << /GSop 9 0 R >> "
        b"/XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>"
    )
    objs[9] = b"<< /Type /ExtGState /OP true /op true /OPM 1 >>"
    # Left box: the mark alone (the control). Right box: the mark, then the
    # image over it with overprint on.
    objs[4] = stream(
        b"<< >>",
        b"q 1 0 1 0.5 k 20 40 60 120 re f Q\n"
        b"q 1 0 1 0.5 k 120 40 60 120 re f Q\n"
        b"q /GSop gs 60 0 0 120 120 40 cm /Im0 Do Q",
    )
    objs[5] = image_object(cs, bytes([0]) * 64, 1)
    write("duotone2-op-image-over-cmyk-mark.pdf", build(objs))

    return 0


if __name__ == "__main__":
    sys.exit(main())
