#!/usr/bin/env python3
"""Regenerate fixtures/synthetic/overprint/ — a SAMPLED IMAGE painted under
`/OP true` in a `Separation`/`DeviceN` colour space.

WHY THIS EXISTS
---------------
`Pass 130.2` gave a sampled image ISO 32000-1 §11.7.4.3's `CompatibleOverprint`
composite. Until it shipped, `overprint::composite` had exactly one call site —
the path and glyph painter — so an image XObject reached the destination through
an ordinary paint no matter what `/OP` said, and Table 149's third row was never
consulted for one.

Table 149's third row is the only row that is not inert for an image:

| source space | process component **named** in it | process component **not named** |
|---|---|---|
| any process space (row 2, which is where a sampled `DeviceCMYK` lands) | `c_s` | `c_s` |
| `Separation` / `DeviceN` (row 3) | `c_s` | **`c_b` under `OP true`** |

So a `/DeviceN [/Cyan]` image over a black backdrop must leave the backdrop's
`M`, `Y` and `K` standing, and a `/DeviceN [/Cyan /Magenta /Yellow /Black]`
image over the same backdrop must knock all four out. **The colorant LIST, not
the tint values, decides**, and that is what these fixtures isolate.

WHY NOT JUST USE THE PRINT-CONFORMANCE SUITE
--------------------------------------------
Because the suite is not redistributable and is not in this repository (see
the private map directory named in the scrub record). It is the oracle that
found this defect and the oracle that scored the fix — three of its patches
went from FAIL to pass — but a fixture that only exists on one machine cannot
guard a regression on CI. These four files are the committed half.

★ AND THEY TEST SOMETHING THE SUITE CANNOT: the sRGB path. Every suite
overprint patch is PDF/X with an output intent, so every one of them composites
in the four-colorant buffer. `Canvas::fill_image_overprint` has a second arm for
an additive page, and nothing in the suite reaches it. `sep_devicen_op_rgb.pdf`
is that arm's only end-to-end evidence.

WHAT THE FIXTURES ARE
---------------------
Four single-page PDFs, each 100 × 100 pt, each painting the SAME 2 × 2 image
over the SAME cyan backdrop square, varying only what is under test:

| file | page group | image space | `/OP` | what it proves |
|---|---|---|---|---|
| `devicen_op_cmyk.pdf` | `/DeviceCMYK` | `[/Indexed [/DeviceN [/Black] /DeviceCMYK f] 1 …]` | true | row 3 preserves: the cyan backdrop survives under the image |
| `devicen_noop_cmyk.pdf` | `/DeviceCMYK` | the same | **false** | the same image knocks the backdrop out — the control that makes the row above a measurement rather than a coincidence |
| `devicen_all4_op_cmyk.pdf` | `/DeviceCMYK` | `[/Indexed [/DeviceN [/Cyan /Magenta /Yellow /Black] /DeviceCMYK f] 1 …]` | true | a space naming every process colorant is INERT under overprint — renders identically to `devicen_noop_cmyk.pdf` |
| `sep_devicen_op_rgb.pdf` | *(none)* | `[/Indexed [/DeviceN [/Black] /DeviceCMYK f] 1 …]` | true | the sRGB arm of the same composite |

★ THE `/Black`-ONLY VARIANT IS THE ONE THAT DISCRIMINATES, and the reason is
worth stating because a reader will otherwise pick `/Cyan`: the backdrop here is
**100 % cyan**. A `/DeviceN [/Cyan]` image would take `C` from the source and
`M`, `Y`, `K` from the backdrop — and the backdrop's `M`, `Y`, `K` are all zero,
so preserving them changes nothing and a broken renderer scores the same as a
correct one. Naming `/Black` instead puts the ONE colorant the backdrop actually
carries in the *preserved* set, so preservation and knock-out land on visibly
different colours.

The image is 2 × 2 with index 0 in every texel, so its content is a flat colour
and any pixel inside its placement is assertable without worrying about
resampling. Its palette entry is tint **0.0** — no ink — which is the strongest
form of the test: under `/OP true` a zero-tint `/DeviceN [/Black]` source must
leave a *cyan* square (the backdrop, untouched, plus zero black), and under
`/OP false` it must leave a *white* one (every process component knocked to
zero). Those two are 255 levels apart in the green and blue channels, so no
threshold judgement is involved.

PROVENANCE
----------
Authored here, byte by byte, from ISO 32000-1's own object syntax. No
third-party PDF is copied, adapted or consulted (`docs/LEGAL.md` §5), and in
particular **no byte of the print-conformance suite is reproduced** — the
shapes above are read off Table 149, not off any patch. The files are small
enough to read in a text editor, deliberately.

This script is NOT part of the build and is not a workspace member, so it never
enters the dependency graph or `THIRD_PARTY_LICENSES.md`.

USAGE
-----
    python tools/gen-overprint-image-fixtures.py
"""

from __future__ import annotations

import pathlib

OUT = pathlib.Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "overprint"

# The image's samples: 2 x 2, 8 bits per component, one component (the index),
# every texel index 0. A row of a 2-wide 8-bpc indexed image is 2 bytes, and
# §8.9.3 requires each row to begin on a byte boundary, which 2 already is.
SAMPLES = bytes([0, 0, 0, 0])


def build(
    *, colorants: list[str], overprint: bool, subtractive: bool, mark: str = "image"
) -> bytes:
    """Assemble one single-page PDF, cross-reference table and all.

    The writer is hand-rolled rather than delegated to `pdfcer` on purpose:
    a fixture generated by the program under test cannot falsify that program.
    Offsets are accumulated as the objects are emitted, so the xref is built
    from the same list the body is.
    """
    n_colorants = len(colorants)
    names = b"".join(b"/" + c.encode() for c in colorants)
    # The `scn` operands for the path twin: full tint on a SPOT-ONLY space (so
    # the mark has ink to lose), zero elsewhere (so an overprinting process
    # colorant leaves the backdrop and the trap can be read).
    spot_only = not any(
        c.lower() in ("cyan", "magenta", "yellow", "black", "all") for c in colorants
    )
    tints = (b"1 " if spot_only else b"0 ") * n_colorants

    # The tint transform, §8.6.6.5: n inputs (one per declared colorant),
    # four outputs (the DeviceCMYK alternate). A type 2 (exponential
    # interpolation) function takes exactly ONE input, so anything with more
    # than one colorant needs a type 4 PostScript calculator — which is also
    # the honest shape, since a real DeviceN transform is rarely separable.
    #
    # ★ The transform's OUTPUT is deliberately not what decides the test.
    # Table 149 row 3 selects on the colorant NAMES; the tints it then paints
    # into the named channels are read from the OPERANDS by
    # `overprint::authored_tints`, never from this function. Writing an
    # identity-ish transform here keeps the two readings agreeing so that a
    # failure is unambiguous, rather than being a disagreement between them.
    if spot_only and n_colorants == 1:
        # A CHROMATIC transform for the spot-visibility fixtures, and the
        # chroma is the point rather than decoration. The other single-colorant
        # transform below routes its tint to `Black`, so a renderer that lost
        # the tint transform entirely and painted "some ink" would still land on
        # black and satisfy a not-blank assertion. Mapping to C=0.8 M=0.2 Y=0.9
        # K=0 makes the correct answer GREEN-DOMINANT, which no accident
        # produces. Stack trace, since a type 4 function is write-only
        # otherwise: [t] dup -> [t,t] 0.8 mul -> [t,0.8t] exch -> [0.8t,t]
        # dup -> [0.8t,t,t] 0.2 mul -> [0.8t,t,0.2t] exch -> [0.8t,0.2t,t]
        # 0.9 mul -> [0.8t,0.2t,0.9t] 0 -> [C,M,Y,K].
        tint_body = b"{ dup 0.8 mul exch dup 0.2 mul exch 0.9 mul 0 }"
        domain = b"[0 1]"
    elif n_colorants == 1:
        # One input, four outputs: pop the input and push 0 0 0 t back, i.e.
        # route the single tint to `Black` and zero the rest. (`/Cyan`-only
        # would route to C; this fixture family only ever declares `/Black`
        # in the single-colorant case -- see the module docstring for why.)
        tint_body = b"{ 0 exch 0 exch 0 exch 4 -1 roll 3 1 roll }"
        # Stack after `{ }` runs must be C M Y K. Simpler and less clever:
        tint_body = b"{ 0 0 0 4 -1 roll }"
        domain = b"[0 1]"
    else:
        # Four inputs, four outputs, identity.
        tint_body = b"{ }"
        domain = b"[0 1 0 1 0 1 0 1]"

    objects: list[bytes] = []

    def add(body: bytes) -> int:
        objects.append(body)
        return len(objects)

    catalog = add(b"<< /Type /Catalog /Pages 2 0 R >>")
    pages = add(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")

    page_content = (
        # The backdrop: 100 % cyan + 100 % black, DeviceCMYK, opaque. Both
        # are carried so that a `/DeviceN [/Black]` source has a non-zero
        # value in BOTH its preserved set (C) and its source set (K) -- a
        # backdrop that is zero in the channel under test cannot distinguish
        # preservation from knock-out.
        b"1 0 0 1 k\n"
        b"10 10 80 80 re f\n"
        # Place the mark over the middle of it. For the image, `cm` maps
        # §8.9.4's unit square onto a 40 x 40 pt region at (30, 30); for the
        # path twin, the same 40 x 40 region is filled directly. The backdrop
        # stays visible around all four sides, which is what makes a
        # whole-mark miss look different from a colour error.
        b"q\n"
        b"/GS0 gs\n"
        + (
            b"40 0 0 40 30 30 cm\n/Im0 Do\n"
            if mark == "image"
            else b"/CS0 cs " + tints + b"scn\n30 30 40 40 re f\n"
        )
        + b"Q\n"
    )

    group = b"/Group << /S /Transparency /CS /DeviceCMYK >> " if subtractive else b""
    page = add(
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] "
        + group
        + b"/Resources << /XObject << /Im0 5 0 R >> "
        b"/ColorSpace << /CS0 7 0 R >> "
        b"/ExtGState << /GS0 4 0 R >> >> "
        b"/Contents 6 0 R >>"
    )
    # `/OPM 1` throughout: it is inert for every row these fixtures exercise
    # (Table 149 makes modes 0 and 1 differ only in row 1, `DeviceCMYK`
    # specified directly and NOT in a sampled image), and carrying it proves
    # that inertness rather than assuming it.
    gs = add(
        b"<< /Type /ExtGState /OP "
        + (b"true" if overprint else b"false")
        + b" /op "
        + (b"true" if overprint else b"false")
        + b" /OPM 1 >>"
    )
    image = add(
        b"<< /Type /XObject /Subtype /Image /Width 2 /Height 2 "
        b"/BitsPerComponent 8 /ColorSpace 7 0 R "
        b"/Length " + str(len(SAMPLES)).encode() + b" >>\nstream\n"
        + SAMPLES
        + b"\nendstream"
    )
    content = add(
        b"<< /Length " + str(len(page_content)).encode() + b" >>\nstream\n"
        + page_content
        + b"endstream"
    )
    # `[/Indexed base hival lookup]`, §8.6.6.3. `hival` is 1, so the table has
    # TWO entries and index 0 -- the only index the samples use -- is tint 0.
    #
    # ★ TWO ENTRIES AND NOT ONE, deliberately: a one-entry palette makes an
    # out-of-range index and a correct lookup produce the same pixel, so a
    # broken palette walk is indistinguishable from a working one. The same
    # trap `Pass 130.0` recorded.
    lookup = bytes([0] * n_colorants + [255] * n_colorants)
    colorspace = add(
        b"[/Indexed [/DeviceN [" + names + b"] /DeviceCMYK 8 0 R] 1 <"
        + lookup.hex().encode()
        + b">]"
    )
    tint = add(
        b"<< /FunctionType 4 /Domain " + domain + b" /Range [0 1 0 1 0 1 0 1] "
        b"/Length " + str(len(tint_body)).encode() + b" >>\nstream\n"
        + tint_body
        + b"\nendstream"
    )

    # ★ CHECK THE LITERALS, NOT THE COUNTER. `add()` returns 1, 2, 3 ... by
    # construction, so asserting on its return values cannot fail; what can
    # fail is an object NUMBER written into a dictionary above drifting away
    # from the object it names, which renders as a plausible wrong picture
    # rather than an error. `R162`: a vacuous assertion beside a wrong
    # literal reads as verification.
    body = b"".join(objects)
    for ref in (
        b"/Im0 " + str(image).encode() + b" 0 R",
        b"/CS0 " + str(colorspace).encode() + b" 0 R",
        b"/GS0 " + str(gs).encode() + b" 0 R",
        b"/Contents " + str(content).encode() + b" 0 R",
        b"/ColorSpace " + str(colorspace).encode() + b" 0 R",
        b"/DeviceCMYK " + str(tint).encode() + b" 0 R",
    ):
        assert body.count(ref) == 1, (
            f"reference {ref!r} is not in the emitted objects exactly once -- "
            "an object number and the literal that points at it have drifted apart"
        )
    assert (catalog, pages, page) == (1, 2, 3)

    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets = [0]
    for i, obj in enumerate(objects, start=1):
        offsets.append(len(out))
        out += str(i).encode() + b" 0 obj\n" + obj + b"\nendobj\n"

    startxref = len(out)
    n = len(objects) + 1
    out += b"xref\n0 " + str(n).encode() + b"\n"
    out += b"0000000000 65535 f \n"
    for off in offsets[1:]:
        out += f"{off:010d} 00000 n \n".encode()
    out += (
        b"trailer\n<< /Size " + str(n).encode() + b" /Root 1 0 R >>\n"
        b"startxref\n" + str(startxref).encode() + b"\n%%EOF\n"
    )
    return bytes(out)


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    written = {
        "devicen_op_cmyk.pdf": build(
            colorants=["Black"], overprint=True, subtractive=True
        ),
        "devicen_noop_cmyk.pdf": build(
            colorants=["Black"], overprint=False, subtractive=True
        ),
        "devicen_all4_op_cmyk.pdf": build(
            colorants=["Cyan", "Magenta", "Yellow", "Black"],
            overprint=True,
            subtractive=True,
        ),
        "sep_devicen_op_rgb.pdf": build(
            colorants=["Black"], overprint=True, subtractive=False
        ),
        # ★★ THE SELF-CONSISTENCY PAIR, and it is the strongest assertion in
        # this family because it needs no reference render at all.
        #
        # `devicen_op_cmyk.pdf` and `devicen_op_path_cmyk.pdf` differ in
        # exactly one way: one draws the overprinting `/DeviceN [/Black]`
        # colour as a SAMPLED IMAGE, the other as a filled RECTANGLE, over
        # the same backdrop, at the same coordinates, at the same tint.
        # §11.7.4.3 makes no distinction between them — Table 149 is written
        # about a source COLOUR, not a source object — so the two must land
        # on the same pixels.
        #
        # They did not before `Pass 130.2`: the rectangle went through
        # `paint_overprint` and came out cyan, the image went through an
        # ordinary paint and came out white. Nothing but a comparison of the
        # two could have said which one was wrong, which is exactly why the
        # pair is committed rather than a single file with a remembered
        # expected colour (`R215` — the wrong-oracle shape).
        "devicen_op_path_cmyk.pdf": build(
            colorants=["Black"], overprint=True, subtractive=True, mark="path"
        ),
        "devicen_op_path_rgb.pdf": build(
            colorants=["Black"], overprint=True, subtractive=False, mark="path"
        ),
        # ★★ THE SPOT-VISIBILITY PAIR. A `/Separation`-style spot square, no
        # overprint at all, on the two kinds of page. They must BOTH put ink on
        # the sheet.
        #
        "spot_only_noop_cmyk.pdf": build(
            colorants=["SpotInk"], overprint=False, subtractive=True, mark="path"
        ),
        "spot_only_noop_rgb.pdf": build(
            colorants=["SpotInk"], overprint=False, subtractive=False, mark="path"
        ),
    }
    for name, data in written.items():
        (OUT / name).write_bytes(data)
        print(f"wrote {OUT / name}  ({len(data)} bytes)")


if __name__ == "__main__":
    main()
