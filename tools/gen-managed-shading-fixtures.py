#!/usr/bin/env python3
"""gen-managed-shading-fixtures — the oracle for `Pass 243.0`.

WHAT THIS BUILDS, AND WHY IT NEEDS NO REFERENCE RENDER
======================================================
Each fixture paints **one colour three ways on one page**: a flat fill, an
axial (type 2) shading whose two stops are BOTH that colour, and a type 4
free-form triangle mesh whose every vertex is that colour. A correct renderer
lands all three on one pixel value, and on a page that composites in ink on
one probed ink. `tools/gen-mesh-ink-fixtures.py`'s oracle, applied to the two
colour-space families that had a managed route for fills and images and NOT
for shadings.

THE DEFECT IT WAS BUILT FOR
===========================
Three Passes gave fills and images a route through the page's colour bridges
— an `ICCBased` colour through its embedded profile (`199.2`, `214.0`,
`240.0`), a `Lab`/`CalRGB`/`CalGray` colour through the output intent
(`242.0`) — and each left the shading and mesh readers behind, because their
colour is resolved inside `shading.rs`/`mesh.rs` through a bare `ColorSpace`
that never saw the bridge cache. So on the same page, through the same
profile, a gradient's end stop and a flat fill of the same operands were two
colours. `Pass 243.0` routes `ColorRamp::build` and `mesh::read_shade` through
`icc::ColorBridges`, the fill path's ladder in one place.

THE SIX PAGES
=============
| file | space | page group | intent | what agreement proves |
|---|---|---|---|---|
| `icc-rgb-shading-additive.pdf` | `ICCBased` swap-RG profile | none | none | the display bridge in the ramp and the vertex reader; expected `(48,102,205)` |
| `icc-rgb-shading-subtractive.pdf` | same | `/DeviceCMYK` | `dest-cmyk.icc` | the ink bridge in both |
| `icc-rgb-shading-subtractive-no-intent.pdf` | same | `/DeviceCMYK` | none | the display bridge feeding `rgb_to_cmyk` uniformly |
| `lab-shading-subtractive.pdf` | `Lab` D50 | `/DeviceCMYK` | `dest-cmyk.icc` | the PCS bridge in both |
| `lab-shading-subtractive-no-intent.pdf` | `Lab` D50 | `/DeviceCMYK` | none | the control: K-only bridge, all three agree |
| `lab-shading-additive.pdf` | `Lab` D50 | none | none | `xyz_to_srgb` on all three (no route changed here; pins that nothing moved) |

★ The profile, operands, samples and expected values are the ones
`tools/gen-icc-rgb-fixtures.py` and `tools/gen-lab-ink-fixtures.py` already
document: `(0.4, 0.2, 0.8)` through a profile whose red and green colorants
are swapped (`(48,102,205)` managed, `(102,51,204)` unmanaged — R225), and
`Lab (60, 0, 0)`. Both profiles are READ from those directories, not
regenerated, so the three fixture families cannot drift apart.

★★ The shading is a CONSTANT ramp on purpose: two identical stops make every
sample of the ramp the fill's colour, so the swatch can be sampled anywhere
inside and the test measures the ROUTE, not the interpolation. (Interpolation
is pinned elsewhere; a gradient here would make the tolerance a function of
where the probe lands.)

USAGE
=====
    python tools/gen-managed-shading-fixtures.py

Writes into `fixtures/synthetic/managed-shading/`. Rights-cleared by
construction: every byte generated here except the two profiles, whose
provenance is in their own directories' `PROVENANCE.md`.
"""

from __future__ import annotations

import pathlib
import struct
import sys
import zlib

ROOT = pathlib.Path(__file__).resolve().parent.parent / "fixtures" / "synthetic"
OUT = ROOT / "managed-shading"
SWAP_PROFILE = ROOT / "icc-rgb" / "swap-rg.icc"
DEST_PROFILE = ROOT / "icc-rgb" / "dest-cmyk.icc"

FULL = 0xFFFFFFFF


def assemble(objects: list[bytes]) -> bytes:
    out = bytearray(b"%PDF-1.7\n")
    offsets = [0]
    for i, body in enumerate(objects, start=1):
        offsets.append(len(out))
        out += str(i).encode() + b" 0 obj\n" + body + b"\nendobj\n"
    startxref = len(out)
    n = len(objects) + 1
    out += b"xref\n0 " + str(n).encode() + b"\n0000000000 65535 f \n"
    for off in offsets[1:]:
        out += f"{off:010d} 00000 n \n".encode()
    out += (
        b"trailer\n<< /Size " + str(n).encode() + b" /Root 1 0 R >>\n"
        b"startxref\n" + str(startxref).encode() + b"\n%%EOF\n"
    )
    return bytes(out)


def stream(dict_body: bytes, data: bytes) -> bytes:
    return (
        b"<< " + dict_body + b" /Length " + str(len(data)).encode() + b" >>\nstream\n"
        + data
        + b"\nendstream"
    )


def type4_stream(sample_bytes: bytes) -> bytes:
    """Two flag-0 triangles covering the Decode square, every vertex `sample_bytes`.

    MSH16: a flag of 0 reads two MORE vertices whose own flags are present in
    the stream and ignored — written as explicit zeros, never skipped.
    """
    out = bytearray()
    for verts in [
        [(0, 0), (FULL, 0), (0, FULL)],
        [(FULL, 0), (FULL, FULL), (0, FULL)],
    ]:
        for x, y in verts:
            out += struct.pack(">B", 0)
            out += struct.pack(">II", x, y)
            out += sample_bytes
    return bytes(out)


class Family:
    """One colour-space family: how to declare the space, the operands, the
    8-bit samples a mesh vertex carries, and the `/Decode` ranges those
    samples are scaled into."""

    def __init__(self, name, space_obj, operands, samples, decode_ranges, expected_note):
        self.name = name
        self.space_obj = space_obj
        self.operands = operands
        self.samples = samples
        self.decode_ranges = decode_ranges
        self.expected_note = expected_note


def build(family: Family, *, group: bool, intent: bool) -> bytes:
    ops = b" ".join(b"%.10g" % v for v in family.operands)
    # Object numbers: 1 catalog, 2 pages, 3 page, 4 content, 5 colour space
    # (+6 its profile stream for ICC), then shadings, then the intent.
    objs: list[bytes] = []
    objs.append(b"")  # 1 catalog, filled last
    objs.append(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
    objs.append(b"")  # 3 page, filled below
    content = (
        b"q /Cs0 cs " + ops + b" scn 10 20 80 60 re f Q\n"
        b"q 110 20 80 60 re W n /Sh2 sh Q\n"
        b"q 210 20 80 60 re W n /Sh4 sh Q\n"
    )
    objs.append(stream(b"", content))  # 4
    space_ref, extra = family.space_obj(len(objs) + 1)
    objs.extend(extra)  # 5 (+6)
    # The constant axial ramp: a type 2 function whose C0 == C1.
    fn = b"<< /FunctionType 2 /Domain [0 1] /C0 [" + ops + b"] /C1 [" + ops + b"] /N 1 >>"
    sh2 = (
        b"<< /ShadingType 2 /ColorSpace " + space_ref
        + b" /Coords [110 0 190 0] /Extend [true true] /Function " + fn + b" >>"
    )
    objs.append(sh2)
    sh2_ref = b"%d 0 R" % len(objs)
    decode = b" ".join(b"%.10g" % v for v in (210, 290, 20, 80) + family.decode_ranges)
    sh4 = stream(
        b"/ShadingType 4 /ColorSpace " + space_ref
        + b" /BitsPerCoordinate 32 /BitsPerComponent 8 /BitsPerFlag 8"
        b" /Decode [" + decode + b"] /Filter /FlateDecode",
        zlib.compress(type4_stream(bytes(family.samples))),
    )
    objs.append(sh4)
    sh4_ref = b"%d 0 R" % len(objs)
    intent_entry = b""
    if intent:
        dest = DEST_PROFILE.read_bytes()
        assert dest[16:20] == b"CMYK"
        objs.append(stream(b"/N 4", dest))
        dest_ref = b"%d 0 R" % len(objs)
        objs.append(
            b"<< /Type /OutputIntent /S /GTS_PDFX "
            b"/OutputConditionIdentifier (pdfcer synthetic CMYK) "
            b"/DestOutputProfile " + dest_ref + b" >>"
        )
        intent_entry = b"/OutputIntents [%d 0 R] " % len(objs)
    objs[0] = b"<< /Type /Catalog /Pages 2 0 R " + intent_entry + b">>"
    grp = b"/Group << /S /Transparency /CS /DeviceCMYK >> " if group else b""
    objs[2] = (
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 100] " + grp
        + b"/Resources << /ColorSpace << /Cs0 " + space_ref + b" >> "
        b"/Shading << /Sh2 " + sh2_ref + b" /Sh4 " + sh4_ref + b" >> >> /Contents 4 0 R >>"
    )
    return assemble(objs)


def icc_space(first: int):
    profile = SWAP_PROFILE.read_bytes()
    assert profile[16:20] == b"RGB "
    return b"%d 0 R" % first, [b"[/ICCBased %d 0 R]" % (first + 1), stream(b"/N 3", profile)]


def lab_space(first: int):
    return b"%d 0 R" % first, [
        b"[/Lab << /WhitePoint [0.9642 1.0 0.8249] /Range [-128 127 -128 127] >>]"
    ]


FAMILIES = [
    Family(
        "icc-rgb",
        icc_space,
        (0.4, 0.2, 0.8),
        (102, 51, 204),
        (0, 1, 0, 1, 0, 1),
        "managed (48,102,205), unmanaged (102,51,204)",
    ),
    Family(
        "lab",
        lab_space,
        (60.0, 0.0, 0.0),
        (153, 128, 128),
        (0, 100, -128, 127, -128, 127),
        "Lab (60,0,0): press separation is chromatic, rgb_to_cmyk is K-only",
    ),
]


def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    print(f"writing to {OUT}")
    for fam in FAMILIES:
        for suffix, group, intent in [
            ("additive", False, False),
            ("subtractive", True, True),
            ("subtractive-no-intent", True, False),
        ]:
            name = f"{fam.name}-shading-{suffix}.pdf"
            data = build(fam, group=group, intent=intent)
            (OUT / name).write_bytes(data)
            print(f"  {name}  {len(data)} bytes   [{fam.expected_note}]")
    return 0


if __name__ == "__main__":
    sys.exit(main())
