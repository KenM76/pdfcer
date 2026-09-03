#!/usr/bin/env python3
"""Regenerate crates/pdfcer-core/src/image_codec/fixtures_jpx.rs.

WHY THIS EXISTS
---------------
`pdfcer-core`'s JPXDecode adapter (ISO 32000-1 §7.4.9, ITU-T T.800) needs
end-to-end tests over *real* JPEG 2000 codestreams. Unit tests over the
adapter's dictionary handling prove that Table 89's inverted rules are
wired up, but only an actual T.800 codestream proves the three things
that can only go wrong at the byte level:

  1. **Channel interleaving.** `hayro-jpeg2000` returns one planar
     `f32` buffer per component; pdfcer interleaves them. An off-by-one
     in that loop produces a colour-shifted image, not a crash.
  2. **Bit-depth normalization.** §7.4.9 allows 1..38 bits per component
     and permits components to differ. pdfcer delivers 8-bit samples,
     range-scaled. Only a >8-bit fixture proves the scale is
     full-range (2^d-1 -> 255) rather than a high-byte truncation.
  3. **Alpha splitting.** `/SMaskInData` (Table 89) decides whether the
     codestream's opacity channel is used at all. The opacity channel
     arrives interleaved with the colour channels and must be lifted
     out of them, never left in the colour samples.

Those codestreams cannot be downloaded. `docs/LEGAL.md` §5 permits only
synthetic or rights-cleared test data, and the obvious sources — the
OpenJPEG conformance suite, pdf.js's JPX regression files, the JPEG
committee's own test images — are all third-party files of unknown or
restricted provenance. They are therefore GENERATED here from pixel
patterns this project authored, and embedded as byte arrays so the tests
stay hermetic.

This script is NOT part of the build. It is run by hand when the fixture
set needs to change, and its output is committed. It is not a Cargo
workspace member, so it never enters the dependency graph or
THIRD_PARTY_LICENSES.md.

USAGE
-----
    python tools/gen-jpx-fixtures.py
    cargo fmt --all

The `cargo fmt` is not optional and not cosmetic: this script wraps the
byte arrays at a fixed column, `rustfmt` wraps `u8` array literals at 16
elements per line, and `cargo fmt --check` is a shipping gate. The
committed output is therefore the rustfmt-normalized form, exactly as
`tools/gen-jpeg-fixtures.py`'s and `tools/gen-bilevel-fixtures.py`'s are.

Requires Pillow built with OpenJPEG (a developer-machine dependency only
— never a pdfcer dependency; pdfcer's own decoder is the pure-Rust
`hayro-jpeg2000`, decision 005 §4.5). Verified with Pillow 12.1.0 /
OpenJPEG 2.5.4.

HOW THE BYTES ARE PRODUCED
--------------------------
Pillow's JPEG2000 plugin wraps **OpenJPEG's encoder**. Every fixture is
written with `irreversible=False, quality_mode='lossless'`, which selects
the reversible 5/3 wavelet and no quantization, so the decoded samples
are bit-exact equal to the source pixels. That exactness is what lets the
expected-output constants below be asserted rather than eyeballed: the
generator decodes each fixture back through Pillow and refuses to write
the file if the round trip is not identity.

Using OpenJPEG as the *encoder* while `hayro-jpeg2000` is the *decoder*
is deliberate and is the strongest available cross-check short of a
second decoder: the two implementations share no code, so a fixture that
OpenJPEG writes and hayro reads to the same pixels has exercised the
format rather than one project's interpretation of it.

TWO CONTAINER SHAPES, BOTH REQUIRED
-----------------------------------
§7.4.9 says "the JPXDecode filter shall expect to read a full JPX file
structure", i.e. a JP2/JPX *box* container, not a bare codestream. Real
producers emit both. `hayro-jpeg2000`'s `Image::new` sniffs the first
bytes and accepts either (`JP2_MAGIC` = 00 00 00 0C 6A 50 20 20, or
`CODESTREAM_MAGIC` = FF 4F FF 51). So the fixture set carries the *same
picture* in both shapes, and the Rust tests assert they decode
identically — which is the only way to prove the sniff is wired up and
that pdfcer is not accidentally depending on the container for geometry
or colour.

Pillow chooses the shape from the output *filename extension*, not from
the `codec=` keyword (a documented-but-inert argument when saving to a
file object). The raw-codestream fixture is therefore produced by
writing a real `.j2k` file to a temporary directory and reading it back.

WHAT IT PRODUCES, AND WHY EACH ONE
----------------------------------
    JPX_GRAY_8_SAMPLES     Expected samples for the two grayscale
                           fixtures: 16 x 4, one 8-bit component.
    JPX_GRAY_8_JP2         The grayscale picture, JP2 box container.
    JPX_GRAY_8_J2K         The same picture, RAW codestream (FF 4F).
    JPX_RGB_8_SAMPLES      Expected samples for the RGB fixture.
    JPX_RGB_8_JP2          4 x 2 sRGB, three 8-bit components.
    JPX_RGBA_8_SAMPLES     Expected COLOUR samples for the RGBA fixture
                           — three components, alpha already removed.
    JPX_RGBA_8_ALPHA       Expected ALPHA samples for the same fixture.
    JPX_RGBA_8_JP2         4 x 2 sRGB + opacity channel. Carries a
                           `cdef` box declaring channel 3 as type 1
                           (opacity), which is what `has_alpha()` reads
                           and therefore what /SMaskInData acts on.
    JPX_GRAY_16_SAMPLES    Expected samples after the 16 -> 8 bit
                           full-range scale.
    JPX_GRAY_16_JP2        4 x 2 grayscale at 16 bits per component.
    JPX_CMYK_8_SAMPLES     Expected samples for the CMYK fixture.
    JPX_CMYK_8_JP2         4 x 2 CMYK — enumerated colour space 12,
                           which §7.4.9 requires a PDF reader to support
                           even though it is outside JPX baseline.
"""

import io
import math
import struct
import tempfile
import textwrap
from pathlib import Path

from PIL import Image

REPO = Path(__file__).resolve().parent.parent
OUT = REPO / "crates" / "pdfcer-core" / "src" / "image_codec" / "fixtures_jpx.rs"
PDF_OUT = REPO / "fixtures" / "synthetic" / "jpx"

# Lossless every time. `irreversible=False` selects the reversible 5/3
# wavelet (T.800 Annex F); `quality_mode='lossless'` suppresses
# quantization. Together they make decode(encode(x)) == x, which every
# expected-output constant in this file depends on.
LOSSLESS = dict(irreversible=False, quality_mode="lossless")


# ---------------------------------------------------------------------------
# The pictures
# ---------------------------------------------------------------------------

GRAY_W, GRAY_H = 16, 4

# A 16 x 4 grayscale ramp-and-edge pattern. Chosen so that every row
# exercises something different and so no two rows are equal (a stride
# bug that reads row n for row n+1 must be visible):
#   row 0  a hard black/white edge in the middle of a byte-aligned run
#   row 1  the mirror of row 0
#   row 2  a monotone ramp, which is what a wavelet coder compresses
#          best and therefore the row most likely to expose a rounding
#          difference if the transform were ever lossy
#   row 3  isolated extremes against mid-grey, the worst case for the
#          5/3 lifting steps at the row boundary
GRAY_ROWS = [
    [0] * 8 + [255] * 8,
    [255] * 8 + [0] * 8,
    [i * 17 for i in range(16)],
    [128, 0, 128, 255, 128, 0, 128, 255, 128, 0, 128, 255, 128, 0, 128, 255],
]

RGB_W, RGB_H = 4, 2
# Primaries plus white on the top row, secondaries plus black on the
# bottom. Every channel takes both extremes, and no two pixels share a
# component triple, so a channel swap or an interleave off-by-one cannot
# produce a coincidentally-equal buffer.
RGB_PIXELS = [
    (255, 0, 0),
    (0, 255, 0),
    (0, 0, 255),
    (255, 255, 255),
    (0, 255, 255),
    (255, 0, 255),
    (255, 255, 0),
    (0, 0, 0),
]

# The same colours with an alpha ramp covering both extremes and two
# intermediate values. Alpha 0 on an otherwise-saturated pixel is the
# case that catches an adapter which left the opacity channel
# interleaved in the colour samples: the colour would still be there,
# but shifted one component to the right.
RGBA_PIXELS = [
    (255, 0, 0, 255),
    (0, 255, 0, 170),
    (0, 0, 255, 85),
    (255, 255, 255, 0),
    (0, 255, 255, 0),
    (255, 0, 255, 85),
    (255, 255, 0, 170),
    (0, 0, 0, 255),
]

GRAY16_W, GRAY16_H = 4, 2
# 16-bit values chosen so the 16 -> 8 full-range scale
# (round(v / 65535 * 255)) lands on exactly-representable answers AND on
# one value (0x0101 = 257) that a high-byte truncation would get wrong:
# truncation gives 0x01, the correct full-range scale gives 1 as well —
# so 0x00FF is included too, where truncation gives 0 and the correct
# scale gives 1. That single pixel is the whole point of this fixture.
GRAY16_PIXELS = [0, 65535, 32768, 257, 255, 65280, 1, 43690]

CMYK_W, CMYK_H = 4, 2
# Each of C, M, Y, K taken to its extreme alone, then three mixtures.
# The fourth component is what distinguishes CMYK from RGBA at the
# channel-count level, and enumerated colour space 12 is what
# distinguishes it in the `colr` box.
CMYK_PIXELS = [
    (255, 0, 0, 0),
    (0, 255, 0, 0),
    (0, 0, 255, 0),
    (0, 0, 0, 255),
    (255, 255, 0, 0),
    (0, 255, 255, 64),
    (128, 128, 128, 128),
    (0, 0, 0, 0),
]


# ---------------------------------------------------------------------------
# Encoding
# ---------------------------------------------------------------------------


def _jp2(img):
    """Encode `img` as a JP2 box container and assert the round trip."""
    buf = io.BytesIO()
    img.save(buf, format="JPEG2000", **LOSSLESS)
    data = buf.getvalue()
    assert data.startswith(b"\x00\x00\x00\x0cjP  "), "not a JP2 signature box"
    _assert_lossless(img, data)
    return data


def _j2k(img):
    """Encode `img` as a RAW codestream and assert the round trip.

    Pillow selects the container from the output filename's extension,
    so this writes a real `.j2k` file to a temporary directory rather
    than passing `codec=` (which is inert for file-object saves).
    """
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "fixture.j2k"
        img.save(path, **LOSSLESS)
        data = path.read_bytes()
    assert data.startswith(b"\xff\x4f\xff\x51"), "not an SOC+SIZ codestream"
    _assert_lossless(img, data)
    return data


def _assert_lossless(img, data):
    """Decode `data` back and require it to equal `img` exactly.

    This is the assertion that lets every expected-output constant below
    be derived from the *source* pixels rather than from whatever the
    codec happened to produce. If OpenJPEG's lossless mode ever stops
    being lossless — or if these encoder settings ever stop selecting it
    — the generator fails here instead of silently committing fixtures
    whose "expected" bytes are really just observed bytes.
    """
    back = Image.open(io.BytesIO(data))
    back.load()
    assert back.size == img.size, f"size changed: {img.size} -> {back.size}"
    assert back.tobytes() == img.tobytes(), "JPEG 2000 round trip was not lossless"


def _gray_image():
    img = Image.new("L", (GRAY_W, GRAY_H))
    img.putdata([v for row in GRAY_ROWS for v in row])
    return img


def _rgb_image():
    img = Image.new("RGB", (RGB_W, RGB_H))
    img.putdata(RGB_PIXELS)
    return img


def _rgba_image():
    img = Image.new("RGBA", (RGB_W, RGB_H))
    img.putdata(RGBA_PIXELS)
    return img


def _gray16_image():
    img = Image.new("I;16", (GRAY16_W, GRAY16_H))
    img.putdata(GRAY16_PIXELS)
    return img


def _cmyk_image():
    img = Image.new("CMYK", (CMYK_W, CMYK_H))
    img.putdata(CMYK_PIXELS)
    return img


def _scale_to_8(value, bit_depth):
    """pdfcer's documented JPX bit-depth normalization, in Python.

    `crates/pdfcer-core/src/image_codec/jpx.rs` delivers 8-bit samples
    range-scaled from the codestream's declared depth:

        out = round(sample / (2^d - 1) * 255)

    NOT a high-byte truncation. Table 89 makes the bit depth "determined
    by the conforming reader in the process of decoding", so 8 is a
    conforming choice; full-range scaling is the one that maps the
    codestream's white point (2^d - 1) onto 255 exactly for every d.

    `floor(x + 0.5)` rather than Python's `round()` on purpose: Rust's
    `f32::round` rounds half **away from zero**, Python's `round` rounds
    half to **even**, and a fixture generated with one rounding rule and
    checked against the other would disagree on any sample that lands
    exactly on .5. Matching the Rust semantics here keeps the two
    definitions of "the expected byte" identical by construction.
    """
    span = (1 << bit_depth) - 1

    return int(math.floor(value * 255.0 / span + 0.5))


def _find_box(data, box_type):
    """Return the payload of the first `box_type` box inside `jp2h`."""
    pos = 0
    while pos + 8 <= len(data):
        (length,) = struct.unpack(">I", data[pos : pos + 4])
        kind = data[pos + 4 : pos + 8]
        if length == 0:
            length = len(data) - pos
        if kind == b"jp2h":
            inner = pos + 8
            end = pos + length
            while inner + 8 <= end:
                (ilen,) = struct.unpack(">I", data[inner : inner + 4])
                ikind = data[inner + 4 : inner + 8]
                if ilen < 8:
                    break
                if ikind == box_type:
                    return data[inner + 8 : inner + ilen]
                inner += ilen
        pos += length
    return None


def build():
    """Every fixture, keyed by the Rust constant name."""
    fx = {}

    gray = _gray_image()
    fx["JPX_GRAY_8_SAMPLES"] = bytes(v for row in GRAY_ROWS for v in row)
    fx["JPX_GRAY_8_JP2"] = _jp2(gray)
    fx["JPX_GRAY_8_J2K"] = _j2k(gray)

    rgb = _rgb_image()
    fx["JPX_RGB_8_SAMPLES"] = bytes(c for px in RGB_PIXELS for c in px)
    fx["JPX_RGB_8_JP2"] = _jp2(rgb)

    rgba = _rgba_image()
    fx["JPX_RGBA_8_SAMPLES"] = bytes(c for px in RGBA_PIXELS for c in px[:3])
    fx["JPX_RGBA_8_ALPHA"] = bytes(px[3] for px in RGBA_PIXELS)
    rgba_data = _jp2(rgba)
    # The `cdef` box is what makes this fixture a /SMaskInData test
    # rather than a four-colour-component test: without it the decoder
    # sees four ordinary channels and reports no alpha at all.
    cdef = _find_box(rgba_data, b"cdef")
    assert cdef is not None, "OpenJPEG wrote no cdef box — RGBA fixture is inert"
    count = struct.unpack(">H", cdef[0:2])[0]
    assert count == 4, f"cdef declares {count} channels, expected 4"
    # Entry layout (JP2 I.5.3.6): channel index, channel type,
    # association. Type 1 is "opacity"; the last entry must carry it.
    idx, typ, assoc = struct.unpack(">HHH", cdef[2 + 6 * 3 : 2 + 6 * 4])
    assert (idx, typ) == (3, 1), f"cdef channel 3 is type {typ}, expected 1 (opacity)"
    fx["JPX_RGBA_8_JP2"] = rgba_data

    gray16 = _gray16_image()
    fx["JPX_GRAY_16_SAMPLES"] = bytes(_scale_to_8(v, 16) for v in GRAY16_PIXELS)
    fx["JPX_GRAY_16_JP2"] = _jp2(gray16)

    cmyk = _cmyk_image()
    fx["JPX_CMYK_8_SAMPLES"] = bytes(c for px in CMYK_PIXELS for c in px)
    cmyk_data = _jp2(cmyk)
    colr = _find_box(cmyk_data, b"colr")
    assert colr is not None, "no colr box in the CMYK fixture"
    # colr payload: METH u8, PREC u8, APPROX u8, then (METH == 1) the
    # 4-byte enumerated colour space. 12 is CMYK — the value §7.4.9
    # singles out as required in PDF despite being outside JPX baseline.
    assert colr[0] == 1, "CMYK fixture does not use an enumerated colour space"
    (enumerated,) = struct.unpack(">I", colr[3:7])
    assert enumerated == 12, f"colr says {enumerated}, expected 12 (CMYK)"
    fx["JPX_CMYK_8_JP2"] = cmyk_data

    return fx


# ---------------------------------------------------------------------------
# Emitting the Rust file
# ---------------------------------------------------------------------------

DOC = {
    "JPX_GRAY_8_SAMPLES": [
        "The expected decoded samples for [`JPX_GRAY_8_JP2`] and",
        "[`JPX_GRAY_8_J2K`]: 16 x 4, one 8-bit component, one byte per",
        "sample.",
        "",
        "A single-component 8-bit image has an identical layout in every",
        "filter pdfcer implements, so this constant is also the check that",
        "the JPX adapter did not accidentally introduce padding, a stride,",
        "or a row flip that the other codecs do not have.",
    ],
    "JPX_GRAY_8_JP2": [
        "16 x 4 grayscale, 8 bits per component, in a **JP2 box",
        "container** — the shape §7.4.9 describes when it says the filter",
        "\"shall expect to read a full JPX file structure\".",
        "",
        "Encoded losslessly by OpenJPEG through Pillow; see",
        "`tools/gen-jpx-fixtures.py`. Must decode to",
        "[`JPX_GRAY_8_SAMPLES`].",
    ],
    "JPX_GRAY_8_J2K": [
        "The **same picture** as [`JPX_GRAY_8_JP2`], as a **raw",
        "codestream** (SOC + SIZ, `FF 4F FF 51`) with no JP2 boxes at all.",
        "",
        "§7.4.9 describes the box-container shape, but real producers embed",
        "bare codestreams and `hayro-jpeg2000` accepts both by sniffing the",
        "first bytes. The two fixtures must decode to the identical",
        "[`JPX_GRAY_8_SAMPLES`], which is what proves pdfcer reads geometry",
        "and colour from the codestream rather than from the container.",
        "",
        "Note that a raw codestream carries NO colour specification box, so",
        "the colour model here comes from §7.4.9's terminal fallback —",
        "1 channel means `DeviceGray`.",
    ],
    "JPX_RGB_8_SAMPLES": [
        "The expected decoded samples for [`JPX_RGB_8_JP2`]: 4 x 2, three",
        "8-bit components, **interleaved** R,G,B per pixel (§8.9.3).",
        "",
        "`hayro-jpeg2000` returns one planar buffer per component; pdfcer",
        "interleaves them. Primaries, secondaries, white and black are all",
        "present and no two pixels share a component triple, so a channel",
        "swap or an interleave off-by-one cannot produce these bytes by",
        "accident.",
    ],
    "JPX_RGB_8_JP2": [
        "4 x 2 sRGB, 8 bits per component, JP2 box container with an",
        "enumerated `colr` box (value 16, sRGB).",
        "",
        "Must decode to [`JPX_RGB_8_SAMPLES`].",
    ],
    "JPX_RGBA_8_SAMPLES": [
        "The expected **colour** samples for [`JPX_RGBA_8_JP2`] — three",
        "components per pixel, with the opacity channel already lifted out.",
        "",
        "This is the constant that catches an adapter which left the",
        "opacity channel interleaved: the colours would all still be",
        "present, merely shifted one component to the right, which is",
        "exactly the kind of failure that looks plausible in a thumbnail.",
    ],
    "JPX_RGBA_8_ALPHA": [
        "The expected **opacity** samples for [`JPX_RGBA_8_JP2`] — one",
        "8-bit component per pixel, the shape",
        "`CodedImage::embedded_alpha` carries.",
        "",
        "Only reachable with `/SMaskInData 1`. Table 89's default is **0**,",
        "which means \"encoded soft-mask image information shall be",
        "ignored\" — so a decoder that always hands back the alpha it found",
        "is wrong, and the pair of tests over this fixture pins both",
        "directions.",
    ],
    "JPX_RGBA_8_JP2": [
        "4 x 2 sRGB **plus an opacity channel**, JP2 box container.",
        "",
        "The `cdef` box (JP2 I.5.3.6) declares channel 3 as type 1,",
        "\"opacity\" — the generator asserts that, because without the box",
        "the decoder would see four ordinary colour channels and report no",
        "alpha, making the fixture silently inert. §7.4.9's channel model",
        "calls this an *ordinary* opacity channel, which is the one",
        "`/SMaskInData 1` selects; the *premultiplied* type that",
        "`/SMaskInData 2` describes is a different channel type that",
        "`hayro-jpeg2000` does not parse (see `jpx.rs`).",
        "",
        "Alpha runs 255, 170, 85, 0, 0, 85, 170, 255 — both extremes and",
        "two intermediate values, so a wrong scale or an inverted channel",
        "cannot pass.",
    ],
    "JPX_GRAY_16_SAMPLES": [
        "The expected decoded samples for [`JPX_GRAY_16_JP2`] **after",
        "pdfcer's 16 -> 8 bit normalization**.",
        "",
        "pdfcer delivers JPX samples at 8 bits per component, range-scaled",
        "`round(v / (2^d - 1) * 255)`. The fixture's 0x00FF pixel is the",
        "discriminator: full-range scaling gives **1**, a high-byte",
        "truncation would give **0**. Table 89 makes the bit depth",
        "\"determined by the conforming reader in the process of",
        "decoding\", so 8 is a conforming choice — but only the full-range",
        "scale maps the codestream's white point onto 255 for every depth.",
    ],
    "JPX_GRAY_16_JP2": [
        "4 x 2 grayscale at **16 bits per component** — the fixture that",
        "exercises §7.4.9's \"any value from 1 to 38 shall be allowed\".",
        "",
        "Must decode to [`JPX_GRAY_16_SAMPLES`], i.e. to 8-bit samples, and",
        "`CodedImage::bits_per_component` must report **8** rather than 16:",
        "the field describes the samples pdfcer delivers, not the depth the",
        "codestream stored them at.",
    ],
    "JPX_CMYK_8_SAMPLES": [
        "The expected decoded samples for [`JPX_CMYK_8_JP2`]: 4 x 2, four",
        "8-bit components interleaved C,M,Y,K.",
        "",
        "Raw ink values, with no `/Decode` applied and no inversion of any",
        "kind (rules R26/R29) — the same contract the DCT adapter honours",
        "for its CMYK output.",
    ],
    "JPX_CMYK_8_JP2": [
        "4 x 2 CMYK, JP2 box container with **enumerated colour space 12**.",
        "",
        "§7.4.9 singles this value out: enumerated colour space 12 (CMYK)",
        "\"is part of JPX but not JPX baseline\" and \"shall be supported",
        "in a PDF\" regardless. The generator asserts the `colr` box really",
        "says 12, so the fixture cannot degrade into a four-channel",
        "`Unknown` image without the test noticing.",
    ],
}

HEADER = '''//! # Synthetic JPEG 2000 codestreams for the JPXDecode adapter's tests
//!
//! **GENERATED FILE — do not hand-edit.** Regenerate with:
//!
//! ```text
//! python tools/gen-jpx-fixtures.py
//! ```
//!
//! ## Provenance (docs/LEGAL.md §5)
//!
//! Every byte array below was produced on a developer machine by
//! `tools/gen-jpx-fixtures.py` from pixel patterns **authored for this
//! project**. Nothing here was downloaded. The obvious sources — the
//! OpenJPEG conformance suite, pdf.js's JPX regression files, the JPEG
//! committee's own test images — are third-party files of unknown or
//! restricted provenance, which `LEGAL.md` §5 forbids outright.
//! Generating them is not a convenience, it is the only permitted route.
//!
//! ## Why OpenJPEG encodes what `hayro-jpeg2000` decodes
//!
//! The fixtures are written by **OpenJPEG** (through Pillow's JPEG2000
//! plugin) and read back by **`hayro-jpeg2000`**, two implementations
//! that share no code. That is the strongest cross-check available short
//! of running a second decoder: a fixture that one project writes and
//! the other reads to the expected pixels has exercised T.800 itself,
//! not one project's reading of it.
//!
//! Every fixture is encoded with the reversible 5/3 wavelet and no
//! quantization (`irreversible=False, quality_mode='lossless'`), and the
//! generator refuses to emit a fixture whose OpenJPEG round trip is not
//! bit-exact. So the `*_SAMPLES` constants are derived from the **source
//! pixels**, not from whatever the codec happened to produce — the
//! difference between an assertion and a recording.
//!
//! ## Why byte arrays rather than files
//!
//! Hermetic tests, exactly as in the sibling `fixtures` and
//! `fixtures_bilevel` modules: embedding the bytes means the tests run
//! identically from any working directory, under `cargo test`, under
//! `cargo fuzz`, and in a `wasm32` check.

// Shared by TWO test suites — `pdfcer-core`'s codec tests and
// `pdfcer-render`'s rasterizer tests (which pull this file in with
// `#[path]`) — and neither uses the whole set.
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


# ---------------------------------------------------------------------------
# Single-page demo PDFs (fixtures/synthetic/jpx/)
# ---------------------------------------------------------------------------
#
# Not needed by the unit tests — those embed the byte arrays above and
# stay hermetic. These exist for the two things a byte array cannot
# serve:
#
#   1. a **seed corpus** for the `image_codec_jpx` fuzz target, which
#      decision 005 §6.5 requires to come from `fixtures/synthetic/` and
#      never from a downloaded real-world PDF (`docs/LEGAL.md` §5);
#   2. an **end-to-end demo** — `pdfcer render-page` on a real file,
#      the only check that exercises the whole path from xref parsing
#      through the content stream to the rasterizer.


def _pdf(objects, root=1):
    """Assemble a classic-xref PDF from 1-based object bodies."""
    buf = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets = []
    for i, body in enumerate(objects, start=1):
        offsets.append(len(buf))
        buf += f"{i} 0 obj\n".encode("ascii") + body + b"\nendobj\n"
    xref_at = len(buf)
    buf += f"xref\n0 {len(objects) + 1}\n".encode("ascii")
    buf += b"0000000000 65535 f \n"
    for off in offsets:
        buf += f"{off:010} 00000 n \n".encode("ascii")
    buf += (
        f"trailer\n<< /Size {len(objects) + 1} /Root {root} 0 R >>\n"
        f"startxref\n{xref_at}\n%%EOF\n"
    ).encode("ascii")
    return bytes(buf)


def _stream(dict_entries, data):
    return (
        f"<< {dict_entries} /Length {len(data)} >>\nstream\n".encode("ascii")
        + data
        + b"\nendstream"
    )


DRAW = "q 100 0 0 100 14 14 cm /Im0 Do Q"


def _page_pdf(image_dict, image_data):
    """One 128 x 128-point page with the image drawn over a 100 pt square."""
    objs = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 128 128] "
        b"/Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>",
        _stream("", DRAW.encode("ascii")),
        _stream(image_dict, image_data),
    ]
    return _pdf(objs)


def build_pdfs(fx):
    """The demo/seed PDFs. Keys are file names."""
    out = {}
    # /ColorSpace and /BitsPerComponent DELIBERATELY ABSENT: Table 89
    # makes both optional for JPXDecode, and a reader that hard-requires
    # them wrongly rejects this conformant file. That is the whole point
    # of this fixture.
    out["jpx-gray-nocs.pdf"] = _page_pdf(
        "/Type /XObject /Subtype /Image /Width 16 /Height 4 /Filter /JPXDecode",
        fx["JPX_GRAY_8_JP2"],
    )
    # A raw codestream instead of a JP2 container, otherwise identical.
    out["jpx-gray-raw-codestream.pdf"] = _page_pdf(
        "/Type /XObject /Subtype /Image /Width 16 /Height 4 /Filter /JPXDecode",
        fx["JPX_GRAY_8_J2K"],
    )
    # /ColorSpace PRESENT and correct: Table 89 says the dictionary wins
    # and the codestream's colour specification is ignored.
    out["jpx-rgb-cs.pdf"] = _page_pdf(
        "/Type /XObject /Subtype /Image /Width 4 /Height 2 "
        "/ColorSpace /DeviceRGB /Filter /JPXDecode",
        fx["JPX_RGB_8_JP2"],
    )
    # /BitsPerComponent 16 PRESENT AND WRONG, plus a /Decode array that
    # would invert the image if it were honoured. Table 89 says both are
    # ignored for JPXDecode. Rendering this file inverted, or reading it
    # at 16 bits, is the failure this fixture exists to make visible.
    out["jpx-rgb-ignored-entries.pdf"] = _page_pdf(
        "/Type /XObject /Subtype /Image /Width 4 /Height 2 "
        "/ColorSpace /DeviceRGB /BitsPerComponent 16 /Decode [1 0 1 0 1 0] "
        "/Filter /JPXDecode",
        fx["JPX_RGB_8_JP2"],
    )
    # /SMaskInData 1: the codestream's opacity channel is live.
    out["jpx-rgba-smaskindata1.pdf"] = _page_pdf(
        "/Type /XObject /Subtype /Image /Width 4 /Height 2 "
        "/SMaskInData 1 /Filter /JPXDecode",
        fx["JPX_RGBA_8_JP2"],
    )
    out["jpx-cmyk.pdf"] = _page_pdf(
        "/Type /XObject /Subtype /Image /Width 4 /Height 2 /Filter /JPXDecode",
        fx["JPX_CMYK_8_JP2"],
    )
    return out


def main():
    fx = build()
    parts = [HEADER]
    for name, data in fx.items():
        parts.append(emit(name, data))
    OUT.write_text("\n\n".join(parts) + "\n", encoding="utf-8", newline="\n")
    total = sum(len(v) for v in fx.values())
    print(f"wrote {OUT} — {len(fx)} fixtures, {total} bytes")
    print(f"  gray  samples: {fx['JPX_GRAY_8_SAMPLES'].hex()}")
    print(f"  rgb   samples: {fx['JPX_RGB_8_SAMPLES'].hex()}")
    print(f"  rgba  colour : {fx['JPX_RGBA_8_SAMPLES'].hex()}")
    print(f"  rgba  alpha  : {fx['JPX_RGBA_8_ALPHA'].hex()}")
    print(f"  gray16 scaled: {fx['JPX_GRAY_16_SAMPLES'].hex()}")
    print(f"  cmyk  samples: {fx['JPX_CMYK_8_SAMPLES'].hex()}")
    print("  NOW RUN `cargo fmt --all` — rustfmt rewraps u8 arrays at 16/line")

    PDF_OUT.mkdir(parents=True, exist_ok=True)
    for filename, data in build_pdfs(fx).items():
        (PDF_OUT / filename).write_bytes(data)
        print(f"  wrote {PDF_OUT / filename} ({len(data)} bytes)")


if __name__ == "__main__":
    main()
