#!/usr/bin/env python3
"""Generate the fixture that pins "a new /SMask REPLACES the one in force".

WHY THIS EXISTS
===============
ISO 32000-1 Table 58, the `/SMask` row, verbatim:

  "Although the current soft mask is sometimes referred to as a 'soft clip',
  altering it with the `gs` operator COMPLETELY REPLACES the old value with the
  new one, rather than intersecting the two as is done with the current
  clipping path parameter."

pdfcer folds a soft mask into the clip by multiplication -- a sound way to apply
ONE mask and a wrong way to apply two. Before `Pass 192.0` a second `gs /SMask`
with no intervening `q`/`Q` never lifted the first mask out, so the clip became
`mask1 x mask2`.

THE SHAPE THAT MAKES IT COSTLY
==============================
A bevel-and-emboss effect is a highlight and a shadow whose masks are
COMPLEMENTARY gradients. Their product is approximately zero, so the second
layer paints under no coverage and simply vanishes, while the first renders
correctly. "The first masked layer works and the second is missing" was the
reported symptom.

THE FIXTURE
===========
`smask-replaces-not-intersects.pdf` reproduces exactly that shape with hard
halves rather than gradients, so the assertion is a colour test rather than a
tolerance:

  * a white page-sized base;
  * `/GS1 gs` -- soft mask whose luminosity group paints white over the LEFT
    half -- then a full-width RED rectangle;
  * `/GS2 gs` -- soft mask whose luminosity group paints white over the RIGHT
    half -- then a full-width BLUE rectangle;
  * and CRUCIALLY no `q`/`Q` between them, because a save/restore would reset
    the graphics state and hide the defect.

Correct: red on the left, blue on the right.
Before the fix: red on the left, and the right half stays WHITE, because
left-mask x right-mask = 0 everywhere.

★ `smask-single-layer-control.pdf` is the CONTROL: one masked layer only. It
must render identically before and after the fix. Without it, "the second layer
now paints" is consistent with a change that broke single-mask painting too.

Provenance: wholly synthetic, byte-authored, `LEGAL.md` §5 category (a). No
licensed corpus file is involved, and nothing here names one.
"""

import pathlib

OUT = pathlib.Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "transparency"


def assemble(objects: list[bytes], root: int = 1) -> bytes:
    """Header, bodies, classic xref, trailer. Hand-rolled: a fixture generated
    by the program under test cannot falsify that program."""
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
        b"trailer\n<< /Size " + str(n).encode() + b" /Root " + str(root).encode()
        + b" 0 R >>\nstartxref\n" + str(startxref).encode() + b"\n%%EOF\n"
    )
    return bytes(out)


def stream(dict_body: bytes, data: bytes) -> bytes:
    return dict_body.replace(b"@LEN@", str(len(data)).encode()) + b"\nstream\n" + data + b"\nendstream"


def mask_group(x0: int, x1: int) -> bytes:
    """A luminosity group painting WHITE over [x0, x1) and nothing elsewhere.

    With `/BC [0]` the untouched part of the group is black -- luminosity 0 --
    so the mask is 1 on the painted half and 0 on the other.
    """
    content = f"1 g {x0} 0 {x1 - x0} 200 re f".encode()
    return stream(
        b"<< /Type /XObject /Subtype /Form /BBox [0 0 200 200] "
        b"/Group << /S /Transparency /CS /DeviceGray /I true >> "
        b"/Resources << >> /Length @LEN@ >>",
        content,
    )


def two_layers() -> bytes:
    """Two soft-masked fills in ONE q-level. The defect's exact shape."""
    content = (
        b"1 1 1 rg 0 0 200 200 re f\n"
        b"/GS1 gs 1 0 0 rg 0 0 200 200 re f\n"
        b"/GS2 gs 0 0 1 rg 0 0 200 200 re f\n"
    )
    return assemble([
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R "
        b"/Resources << /ExtGState << /GS1 5 0 R /GS2 6 0 R >> >> >>",
        stream(b"<< /Length @LEN@ >>", content),
        b"<< /Type /ExtGState /SMask << /S /Luminosity /G 7 0 R /BC [0] >> >>",
        b"<< /Type /ExtGState /SMask << /S /Luminosity /G 8 0 R /BC [0] >> >>",
        mask_group(0, 100),
        mask_group(100, 200),
    ])


def one_layer() -> bytes:
    """THE CONTROL: a single masked fill. Must be unaffected by the fix."""
    content = b"1 1 1 rg 0 0 200 200 re f\n/GS1 gs 1 0 0 rg 0 0 200 200 re f\n"
    return assemble([
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R "
        b"/Resources << /ExtGState << /GS1 5 0 R >> >> >>",
        stream(b"<< /Length @LEN@ >>", content),
        b"<< /Type /ExtGState /SMask << /S /Luminosity /G 6 0 R /BC [0] >> >>",
        mask_group(0, 100),
    ])


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    for name, data in {
        "smask-replaces-not-intersects.pdf": two_layers(),
        "smask-single-layer-control.pdf": one_layer(),
    }.items():
        (OUT / name).write_bytes(data)
        print(f"wrote {OUT / name}  ({len(data)} bytes)")


if __name__ == "__main__":
    main()
