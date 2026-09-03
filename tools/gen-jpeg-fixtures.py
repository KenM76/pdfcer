#!/usr/bin/env python3
"""Regenerate crates/pdfcer-core/src/image_codec/fixtures.rs.

WHY THIS EXISTS
---------------
`pdfcer-core`'s DCTDecode adapter needs end-to-end tests over *real* JPEG
codestreams: a marker-chain unit test proves the sniff logic, but only an
actual Huffman-coded stream proves that pdfcer asked `zune-jpeg` for the
right output colourspace, sized the buffer correctly, and handed back the
samples in the layout `pdfcer-render` expects.

Those codestreams cannot be downloaded: `docs/LEGAL.md` §5 permits only
synthetic or rights-cleared test data, and a JPEG pulled off the web is
neither. They are therefore GENERATED here, from solid-colour pixel data
this project authored, and embedded as byte arrays so the tests stay
hermetic (no file paths, no working-directory assumptions, no I/O in a
unit test).

This script is NOT part of the build. It is run by hand when the fixture
set needs to change, and its output is committed. It is not a Cargo
workspace member, so it never enters the dependency graph or
THIRD_PARTY_LICENSES.md.

USAGE
-----
    python tools/gen-jpeg-fixtures.py

Requires Pillow (a developer-machine dependency only — never a pdfcer
dependency). Verified with Pillow 12.1.0.

WHAT IT PRODUCES, AND WHY EACH ONE
----------------------------------
    GRAY_2X2              1 component, no APP14. Table 13's
                          "shall be ignored if the image has one or two
                          colour components" case.
    RGB_2X2               3 components, no APP14 → Table 13's default
                          ColorTransform 1 → zune applies YCbCr→RGB.
    RGB_2X2_PROGRESSIVE   SOF2. 14% of the corpus (decision 005 §3.2);
                          "baseline is enough" is false.
    RGB_2X2_APP14_T1      Adobe marker saying 1. Same result as
                          RGB_2X2, but reached through the marker branch
                          of the precedence chain rather than the
                          default branch.
    RGB_2X2_APP14_T0      Adobe marker saying 0 → NO transform. The
                          stored components are still YCbCr (the encoder
                          transformed them), so the decoded samples are
                          those YCbCr values delivered verbatim. That is
                          the point: it proves pdfcer took the passthrough
                          route, and it is the only way to build a
                          transform-0 stream without a bespoke encoder.
    RGB_2X2_APP14_T3      Adobe marker saying 3 — outside Table 13's
                          0..2. zune-jpeg hard-errors on it deep inside
                          header parsing; pdfcer's pre-sniff must turn it
                          into a NAMED diagnostic first (rule R27).
    CMYK_2X2              4 components with an Adobe APP14 marker
                          (transform 0, which is what libjpeg writes for
                          CMYK). Table 13's "0 otherwise" default case
                          and the dct_cmyk_images counter's trigger.
                          NOTE: Pillow INVERTS Adobe CMYK on read; pdfcer
                          deliberately does not (decision 005 §5.5), so
                          pdfcer's samples are the complement of Pillow's.
    RGB_HUGE_DIMS         RGB_2X2 with its SOF width/height patched to
                          65535 × 65535 = 4.3 Gpx. Trips
                          MAX_IMAGE_PIXELS. Cannot be produced by an
                          encoder — a 4.3 Gpx image would have to be
                          allocated to encode it — so it is patched.
"""

import io
import struct
import textwrap
from pathlib import Path

from PIL import Image

REPO = Path(__file__).resolve().parent.parent
OUT = REPO / "crates" / "pdfcer-core" / "src" / "image_codec" / "fixtures.rs"


def jpeg(img, **kw):
    buf = io.BytesIO()
    img.save(buf, format="JPEG", **kw)
    return buf.getvalue()


def insert_app14(data, transform):
    """Splice an Adobe APP14 segment in right after SOI.

    Layout (Adobe TN #5116): 'Adobe'(5) version(2) flags0(2) flags1(2)
    transform(1) = 12 payload bytes, so the length field is 14. It must
    precede the SOF so a decoder sees it before the component count.
    """
    payload = b"Adobe" + b"\x00\x64\x00\x00\x00\x00" + bytes([transform])
    assert len(payload) == 12
    seg = b"\xFF\xEE" + struct.pack(">H", len(payload) + 2) + payload
    return data[:2] + seg + data[2:]


def patch_sof_dims(data, width, height):
    """Rewrite the first SOF header's X and Y fields.

    SOF payload: precision(1) height(2) width(2) components(1) ...
    The 0xC4 / 0xC8 / 0xCC gaps in the 0xC0..0xCF range are DHT, JPG and
    DAC — not frame headers.
    """
    out = bytearray(data)
    i = 2
    while i < len(data) - 1:
        if data[i] != 0xFF:
            break
        marker = data[i + 1]
        i += 2
        if marker in (0xD8, 0x01) or 0xD0 <= marker <= 0xD7:
            continue
        if marker == 0xDA:
            break
        length = struct.unpack(">H", data[i : i + 2])[0]
        if 0xC0 <= marker <= 0xCF and marker not in (0xC4, 0xC8, 0xCC):
            out[i + 3 : i + 5] = struct.pack(">H", height)
            out[i + 5 : i + 7] = struct.pack(">H", width)
            return bytes(out)
        i += length
    raise SystemExit("no SOF marker found")


def build():
    gray = Image.new("L", (2, 2))
    gray.putdata([0, 85, 170, 255])
    rgb = Image.new("RGB", (2, 2))
    rgb.putdata([(255, 0, 0), (0, 255, 0), (0, 0, 255), (255, 255, 255)])
    cmyk = Image.new("CMYK", (2, 2))
    cmyk.putdata([(255, 0, 0, 0), (0, 255, 0, 0), (0, 0, 255, 0), (0, 0, 0, 255)])

    # subsampling=0 (4:4:4) keeps every pixel independent, so a 2x2
    # fixture's four pixels stay distinguishable after the DCT round
    # trip. optimize=True builds minimal Huffman tables, which is what
    # keeps these fixtures a few hundred bytes instead of a kilobyte.
    common = dict(quality=90, optimize=True)
    fx = {}
    fx["GRAY_2X2"] = jpeg(gray, **common)
    fx["RGB_2X2"] = jpeg(rgb, subsampling=0, **common)
    fx["RGB_2X2_PROGRESSIVE"] = jpeg(rgb, subsampling=0, progressive=True, **common)
    fx["RGB_2X2_APP14_T1"] = insert_app14(fx["RGB_2X2"], 1)
    fx["RGB_2X2_APP14_T0"] = insert_app14(fx["RGB_2X2"], 0)
    fx["RGB_2X2_APP14_T3"] = insert_app14(fx["RGB_2X2"], 3)
    fx["CMYK_2X2"] = jpeg(cmyk, subsampling=0, **common)
    fx["RGB_HUGE_DIMS"] = patch_sof_dims(fx["RGB_2X2"], 0xFFFF, 0xFFFF)
    return fx


DOC = {
    "GRAY_2X2": [
        "2 x 2 **grayscale** baseline JPEG (SOF0, 1 component, no APP14).",
        "",
        "Source pixels, left-to-right then top-to-bottom: 0, 85, 170, 255.",
        "Table 13's `ColorTransform` \"shall be ignored if the image has one",
        "or two colour components\", so this fixture must decode identically",
        "whatever `/DecodeParms` says.",
    ],
    "RGB_2X2": [
        "2 x 2 **RGB** baseline JPEG (SOF0, 3 components, 4:4:4, no APP14).",
        "",
        "Source pixels: red, green, blue, white. With no Adobe marker and no",
        "`/ColorTransform`, Table 13's default for three components is **1**,",
        "so the decoder applies the YCbCr -> RGB inverse and the four pixels",
        "come back recognizably red / green / blue / white.",
    ],
    "RGB_2X2_PROGRESSIVE": [
        "The same image encoded **progressively** (SOF2).",
        "",
        "§7.4.8: \"beginning with PDF 1.3, the `DCTDecode` filter shall support",
        "the progressive JPEG extension.\" NOTE 5 calls progressive pointless",
        "for embedded data, and it is still 14% of the corpus (decision 005",
        "§3.2) because PDFs get built from web-sourced images. A baseline-only",
        "decoder leaves one JPEG in seven undrawn.",
    ],
    "RGB_2X2_APP14_T1": [
        "[`RGB_2X2`] with an Adobe APP14 segment declaring **transform 1**.",
        "",
        "Decodes identically to [`RGB_2X2`], but reaches that result through",
        "the *marker* branch of Table 13's precedence chain rather than the",
        "default branch — so the pair together prove the chain's first two",
        "levels agree when they should.",
    ],
    "RGB_2X2_APP14_T0": [
        "[`RGB_2X2`] with an Adobe APP14 segment declaring **transform 0**",
        "(no transformation).",
        "",
        "The stored components are still YCbCr — the encoder transformed them",
        "— so a transform-0 decode delivers those YCbCr values **verbatim**.",
        "That is exactly the point: the decoded samples differ visibly from",
        "[`RGB_2X2`]'s, which is what proves pdfcer took the passthrough route",
        "instead of applying the inverse anyway. Constructing the stream this",
        "way is also the only option without a bespoke encoder, since libjpeg",
        "will not emit untransformed RGB.",
    ],
    "RGB_2X2_APP14_T3": [
        "[`RGB_2X2`] with an Adobe APP14 segment declaring **transform 3**.",
        "",
        "Outside Table 13's 0..2. `zune-jpeg` treats it as a hard error deep",
        "inside header parsing (`headers.rs:485-514`), where the value never",
        "reaches pdfcer's Table 13 logic at all. pdfcer's own APP14 pre-sniff",
        "must catch it first and produce the named `DCT/adobe-transform-3`",
        "diagnostic (rule R27).",
    ],
    "CMYK_2X2": [
        "2 x 2 **CMYK** baseline JPEG (SOF0, 4 components) with an Adobe",
        "APP14 segment — libjpeg writes **transform 0** for CMYK.",
        "",
        "Table 13's \"0 otherwise\" default case, and the trigger for the",
        "`dct_cmyk_images` counter (decision 005 §6.4). **Zero** four-component",
        "JPEGs exist in the 2,914-file conformance corpus, which is why the",
        "Adobe-inversion question is left open (§5.5) and why this fixture is",
        "synthetic rather than measured.",
        "",
        "NOTE: Pillow *inverts* Adobe CMYK JPEGs when reading them. pdfcer",
        "deliberately does not — it passes raw samples through and lets",
        "`/Decode` do its documented job — so pdfcer's samples are the",
        "complement of Pillow's for this fixture. That is the behaviour under",
        "test, not a bug.",
    ],
    "RGB_HUGE_DIMS": [
        "[`RGB_2X2`] with its SOF `X` and `Y` fields patched to",
        "**65535 x 65535** = 4.3 Gpx.",
        "",
        "Trips `MAX_IMAGE_PIXELS` (rule R25). It has to be patched rather than",
        "encoded, because encoding a 4.3 Gpx image would require allocating",
        "one. 65535 is JPEG's own ceiling — T.81's SOF header stores X and Y",
        "as 16-bit integers — so this is the largest geometry any codestream",
        "can claim.",
    ],
}

HEADER = '''//! # Synthetic JPEG codestreams for the DCTDecode adapter's tests
//!
//! **GENERATED FILE — do not hand-edit.** Regenerate with:
//!
//! ```text
//! python tools/gen-jpeg-fixtures.py
//! ```
//!
//! ## Provenance (docs/LEGAL.md §5)
//!
//! Every byte array below was produced on a developer machine by
//! `tools/gen-jpeg-fixtures.py` from **solid-colour pixel data authored
//! for this project** — a 2 x 2 grid of primaries. Nothing here was
//! downloaded, and nothing here derives from a third-party image.
//! `LEGAL.md` §5 permits synthetic or rights-cleared test data only, and
//! a JPEG pulled off the web is neither, so generating them is not a
//! convenience but the only permitted route.
//!
//! Encoder: Pillow 12.1.0 (libjpeg-turbo), quality 90, `optimize=True`
//! (minimal Huffman tables — this is what keeps each fixture a few
//! hundred bytes), 4:4:4 chroma for the colour images so all four pixels
//! of a 2 x 2 fixture survive the DCT round trip distinguishably.
//!
//! ## Why byte arrays rather than files
//!
//! Hermetic tests. A `include_bytes!` of a path under `fixtures/` ties
//! the unit tests to a working directory and to a relative path that
//! breaks the moment the module moves; embedding the bytes means the
//! tests run identically from any cwd, under `cargo test`, under
//! `cargo fuzz`, and in a `wasm32` check.
//!
//! ## What each fixture is for
//!
//! Each constant's own doc comment states the specific §7.4.8 / Table 13
//! rule it exercises, and — where a fixture had to be *constructed*
//! rather than *encoded* — why no encoder can produce it.

// This module is shared by TWO test suites — `pdfcer-core`'s codec tests
// and `pdfcer-render`'s rasterizer tests (which pull it in with
// `#[path]`) — and neither uses the whole set. `dead_code` would
// otherwise fire in whichever crate happens not to need a given fixture,
// which is an argument for splitting the file rather than a real
// warning. One provenance record beats a clean lint here.
#![allow(dead_code)]
'''


def emit(name, data):
    lines = [f"/// {line}".rstrip() for line in DOC[name]]
    body = ", ".join(f"0x{b:02X}" for b in data)
    wrapped = textwrap.fill(
        body, width=76, initial_indent="    ", subsequent_indent="    "
    )
    lines.append(f"pub const {name}: &[u8] = &[")
    lines.append(wrapped)
    lines.append("];")
    return "\n".join(lines)


def main():
    fx = build()
    parts = [HEADER]
    for name, data in fx.items():
        parts.append(emit(name, data))
    OUT.write_text("\n\n".join(parts) + "\n", encoding="utf-8", newline="\n")
    total = sum(len(v) for v in fx.values())
    print(f"wrote {OUT} — {len(fx)} fixtures, {total} bytes of JPEG")


if __name__ == "__main__":
    main()
