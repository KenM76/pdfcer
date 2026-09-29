#!/usr/bin/env python3
"""Regenerate fixtures/synthetic/mesh-ink/ -- a flat fill, a type 4 mesh and a
type 6 patch, all painting the SAME authored ink, side by side.

WHY THIS EXISTS
---------------
A mesh shading's colour is decoded to three-channel sRGB inside the PARSER, one
value per vertex, before any geometry is rasterised. Until `Pass 137.1` there
was nowhere for the authored colorants to live afterwards, so on a page that
composites in ink every mesh took a `CMYK -> sRGB -> CMYK` round trip that a
flat fill of the same colour did not. The return leg is a different function
from the outbound one, so the ink that arrived was not the ink that left.

★ THIS IS THE SECOND HALF OF A DEFECT WHOSE FIRST HALF WAS FIXED SEPARATELY.
`Pass 137.0` gave ANALYTIC shadings (types 2 and 3) their authored ink. The two
mesh patches on the same conformance page were left behind, and after that Pass
they were the ONLY pairs on the page still visibly disagreeing with their own
reference images -- 24.06 and 16.87 mean absolute levels, against 1.16 and 3.52
for the analytic types beside them. Fixing half of something makes the other
half conspicuous; that is an argument for fixing halves, but the leftover half
should not be allowed to sit.

WHAT THE FIXTURES ARE
---------------------
Two single-page PDFs, 300 x 100 pt, each painting the SAME `DeviceCMYK` colour
THREE times:

  x  10..90    a flat filled rectangle          (the control -- never bridged)
  x 110..190   a type 4 free-form triangle mesh (two triangles, flat colour)
  x 210..290   a type 6 Coons patch             (one patch, flat colour)

| file | page group | what it pins |
|---|---|---|
| `mesh-vs-fill-cmyk.pdf` | `/DeviceCMYK` | on an ink page all three must be the SAME colour |
| `mesh-vs-fill-rgb.pdf` | *(none)* | the additive control: they must match there too |

★ WHY BOTH A TRIANGLE MESH AND A PATCH, when one defect caused both.
They reach the ink through DIFFERENT code. A type 4 triangle carries its shades
straight into `fill_triangle`, which interpolates them barycentrically. A type 6
patch carries FOUR CORNER shades that `Patch::shade_at` bilinearly interpolates
into a subdivision grid FIRST, via `Shade::lerp`, and only then hands triangles
to the same rasteriser. A carrier that survives one path and is dropped by the
other would be invisible to a fixture that tested only one -- and the file that
exposed the original defect contains type 7 patches, not triangles, so the patch
path is the one that actually mattered.

★★ WHY THE COLOUR IS FLAT, when a mesh's whole purpose is to vary.
Because the assertion is about colour TRANSPORT, not interpolation. A varying
mesh cannot be compared against a flat fill without choosing a point and then
arguing about whether the chosen pixel is really where you think it is. With
every vertex the same, any pixel of the mesh is comparable to any pixel of the
fill, and a failure means the ink changed on the way -- which is the only thing
being claimed. Interpolation is covered by unit tests in `mesh.rs`, where the
arithmetic can be asserted exactly rather than inferred from pixels.

★★★ AND THE COLOUR IS DELIBERATELY NOT A PRIMARY, AND IS QUANTISED FIRST.
`BitsPerComponent 8` means the mesh's authored value is `round(v * 255) / 255`,
so the flat fill is written with THOSE values rather than the nominal ones --
otherwise the fixture would carry a built-in 1/255 disagreement and its
tolerance would have to be loosened to hide it, which is how a test stops being
able to fail. The chosen ink has all four colorants non-zero and sits nowhere
near a `CMYK <-> sRGB` fixed point: pure cyan or black survives a round trip
nearly intact and would let the defect pass unnoticed.

PROVENANCE
----------
Authored here, byte by byte, from ISO 32000-1's own object syntax (Table 79 for
the shading dictionary, Table 84 for type 4's vertex records, Table 85 for type
6's patch records). No third-party PDF is copied, adapted or consulted
(`docs/LEGAL.md` §5), and in particular nothing from the licensed conformance
suite that exposed the defect: different colour, different page size, no
reference image, a flat colour where that suite uses gradients, and type 4/6
where it uses type 7.

USAGE
-----
    python tools/gen-mesh-ink-fixtures.py
"""

from __future__ import annotations

import pathlib
import struct
import zlib

OUT = pathlib.Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "mesh-ink"

# All four colorants non-zero, far from any CMYK<->sRGB fixed point.
NOMINAL = (0.42, 0.87, 0.13, 0.06)
# What 8-bit components actually decode to. The flat fill uses THESE, so the
# three marks are the same colour by construction rather than to within 1/255.
BYTES_ = tuple(round(v * 255) for v in NOMINAL)
EXACT = tuple(b / 255.0 for b in BYTES_)
INK = " ".join(f"{v:.8f}" for v in EXACT).encode()

FULL = 0xFFFFFFFF
# Raw 32-bit coordinates at 0, 1/3, 2/3 and 1 of the Decode range.
THIRDS = (0, 0x55555555, 0xAAAAAAAA, FULL)

# MSH29 stream order for a type 6 patch: the boundary walked counterclockwise
# from p00. (i, j) with i = column (u), j = row (v). Copied from mesh.rs's
# PATCH_ORDER so the fixture and the parser cannot disagree about the order --
# if one is wrong they are wrong together and the test still detects it, which
# is the honest failure mode for a hand-authored binary fixture.
PATCH_ORDER = [
    (0, 0), (0, 1), (0, 2), (0, 3),
    (1, 3), (2, 3), (3, 3), (3, 2),
    (3, 1), (3, 0), (2, 0), (1, 0),
]


def assemble(objects: list[bytes]) -> bytes:
    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
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


def type4_stream() -> bytes:
    """Two flag-0 triangles covering the unit square of the Decode range.

    MSH16: a flag of 0 means "read two MORE vertices", and those two vertices'
    own flag fields are READ AND IGNORED -- present in the stream, not absent.
    A generator that omits them desynchronises every byte that follows, so they
    are written here as explicit zeros rather than skipped.
    """
    out = bytearray()
    tri = [
        # (0,0) (1,0) (0,1)
        [(0, 0), (FULL, 0), (0, FULL)],
        # (1,0) (1,1) (0,1)
        [(FULL, 0), (FULL, FULL), (0, FULL)],
    ]
    for verts in tri:
        for x, y in verts:
            out += struct.pack(">B", 0)          # flag (ignored on 2nd and 3rd)
            out += struct.pack(">II", x, y)
            out += bytes(BYTES_)
    return bytes(out)


def type6_stream() -> bytes:
    """One flag-0 Coons patch whose boundary is a rectangle with linear edges.

    A Coons surface interpolates its boundary, so a rectangle with straight,
    evenly-spaced edge control points IS the rectangle -- no curvature, and the
    subdivision density therefore cannot change the covered area. That matters:
    a fixture whose coverage moved with the zoom would make the swatch bounds a
    function of `--scale`, and the test would be measuring subdivision rather
    than colour.
    """
    out = bytearray(struct.pack(">B", 0))        # flag 0: a new, independent patch
    for i, j in PATCH_ORDER:
        out += struct.pack(">II", THIRDS[i], THIRDS[j])
    for _ in range(4):                            # c00, c03, c33, c30 -- all equal
        out += bytes(BYTES_)
    return bytes(out)


def build(subtractive: bool) -> bytes:
    content = (
        b"q " + INK + b" k\n10 20 80 60 re f\nQ\n"
        b"q\n110 20 80 60 re W n\n/Sh4 sh\nQ\n"
        b"q\n210 20 80 60 re W n\n/Sh6 sh\nQ\n"
    )
    group = b"/Group << /S /Transparency /CS /DeviceCMYK >> " if subtractive else b""

    def mesh_obj(shading_type: int, x0: int, x1: int, data: bytes) -> bytes:
        packed = zlib.compress(data)
        return stream(
            b"/ShadingType " + str(shading_type).encode()
            + b" /ColorSpace /DeviceCMYK"
            b" /BitsPerCoordinate 32 /BitsPerComponent 8 /BitsPerFlag 8"
            b" /Decode [" + f"{x0} {x1} 20 80".encode() + b" 0 1 0 1 0 1 0 1]"
            b" /Filter /FlateDecode",
            packed,
        )

    return assemble([
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 100] " + group
        + b"/Resources << /Shading << /Sh4 5 0 R /Sh6 6 0 R >> >> /Contents 4 0 R >>",
        stream(b"", content),
        mesh_obj(4, 110, 190, type4_stream()),
        mesh_obj(6, 210, 290, type6_stream()),
    ])


# A spot colorant for `spot_mesh_over_k`: `/SpotGreen` over DeviceCMYK, the
# same tint transform `gen-shading-ink-fixtures.py` uses. The tint is 8-bit
# quantised FIRST (153/255 = 0.6 exactly), so the fill and the meshes state
# the same value.
SPOT_TINT = b"<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [0.9 0.0 0.75 0.1] /N 1 >>"
SPOT_BYTE = 153
SPOT_T = f"{SPOT_BYTE / 255.0:.8f}".encode()


def spot_mesh_over_k() -> bytes:
    """A `/Separation` fill, a per-vertex type 4 mesh and a PARAMETRIC type 6
    patch of the same spot tint, all overprinting a `0 0 0 0.5 k` mark (`Pass
    393.0`).

    THE DISCRIMINATING GEOMETRY, for the reason `gen-shading-ink-fixtures.py`'s
    `over_k` gives: on white paper a deposited spot and a flattened one land
    on the same sRGB by construction. Over 50 % K under `/OP true` they
    separate -- a spot deposited into its own plane leaves the K standing
    (Table 149, spot source x process colorant => c_b), while a flattened one
    is written into C/M/Y/K. The fill beside the meshes deposits, so agreement
    means each mesh took the plane.

    Both mesh carriers are exercised: type 4 carries the tint PER VERTEX (no
    `/Function`), type 6 carries a parametric `t` resolved through a
    `/Function` ramp -- different code on both the parse and the paint side.
    """
    gs = b"/GSop gs "
    content = (
        b"q 0 0 0 0.5 k 10 20 80 60 re f 110 20 80 60 re f 210 20 80 60 re f Q\n"
        b"q " + gs + b"/Cs0 cs " + SPOT_T + b" scn 10 20 80 60 re f Q\n"
        b"q " + gs + b"110 20 80 60 re W n /Sh4 sh Q\n"
        b"q " + gs + b"210 20 80 60 re W n /Sh6 sh Q\n"
    )
    tris = bytearray()
    for verts in ([(0, 0), (FULL, 0), (0, FULL)], [(FULL, 0), (FULL, FULL), (0, FULL)]):
        for x, y in verts:
            tris += struct.pack(">B", 0) + struct.pack(">II", x, y) + bytes([SPOT_BYTE])
    patch = bytearray(struct.pack(">B", 0))
    for i, j in PATCH_ORDER:
        patch += struct.pack(">II", THIRDS[i], THIRDS[j])
    patch += bytes(4)  # t = 0 at all four corners -> the function's C0

    def mesh(shading_type: int, x0: int, x1: int, data: bytes, function: bytes) -> bytes:
        return stream(
            b"/ShadingType " + str(shading_type).encode()
            + b" /ColorSpace 7 0 R" + function
            + b" /BitsPerCoordinate 32 /BitsPerComponent 8 /BitsPerFlag 8"
            b" /Decode [" + f"{x0} {x1} 20 80".encode() + b" 0 1]"
            b" /Filter /FlateDecode",
            zlib.compress(bytes(data)),
        )

    return assemble([
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 100] "
        b"/Group << /S /Transparency /CS /DeviceCMYK >> "
        b"/Resources << /ColorSpace << /Cs0 7 0 R >> /ExtGState << /GSop 9 0 R >> "
        b"/Shading << /Sh4 5 0 R /Sh6 6 0 R >> >> /Contents 4 0 R >>",
        stream(b"", content),
        mesh(4, 110, 190, tris, b""),
        mesh(6, 210, 290, patch, b" /Function 10 0 R"),
        b"[/Separation /SpotGreen /DeviceCMYK 8 0 R]",
        SPOT_TINT,
        b"<< /Type /ExtGState /OP true /op true /OPM 1 >>",
        # The patch's ramp: t -> the one tint, at both ends.
        b"<< /FunctionType 2 /Domain [0 1] /C0 [" + SPOT_T + b"] /C1 [" + SPOT_T
        + b"] /N 1 >>",
    ])


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    for name, data in {
        "mesh-vs-fill-cmyk.pdf": build(subtractive=True),
        "mesh-vs-fill-rgb.pdf": build(subtractive=False),
        "spot-mesh-op-over-k-vs-fill.pdf": spot_mesh_over_k(),
    }.items():
        (OUT / name).write_bytes(data)
        print(f"wrote {OUT / name}  ({len(data)} bytes)")
    print(f"ink: nominal {NOMINAL} -> 8-bit {BYTES_} -> exact {EXACT}")


if __name__ == "__main__":
    main()
