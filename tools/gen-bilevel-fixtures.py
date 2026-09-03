#!/usr/bin/env python3
"""Regenerate crates/pdfcer-core/src/image_codec/fixtures_bilevel.rs.

WHY THIS EXISTS
---------------
`pdfcer-core`'s CCITTFaxDecode (§7.4.6) and JBIG2Decode (§7.4.7) adapters
need end-to-end tests over *real* codestreams. Parameter-mapping unit
tests prove that Table 11's defaults reach `hayro-ccitt`'s
`DecodeSettings`, but only an actual T.4/T.6-coded bit stream proves the
row stride, the byte padding and — above all — the **polarity** are
right. `BlackIs1`'s default is the single most likely correctness bug in
the fax filter: getting it backwards renders every scanned page as its
own negative, which looks deliberate rather than broken.

Those codestreams cannot be downloaded. `docs/LEGAL.md` §5 permits only
synthetic or rights-cleared test data, and the obvious sources — pdf.js's
and PDFBox's JBIG2/CCITT test suites — are third-party files of unknown
provenance. They are therefore GENERATED here from a 16 x 4 pixel pattern
this project authored, and embedded as byte arrays so the tests stay
hermetic.

This script is NOT part of the build. It is run by hand when the fixture
set needs to change, and its output is committed. It is not a Cargo
workspace member, so it never enters the dependency graph or
THIRD_PARTY_LICENSES.md.

USAGE
-----
    python tools/gen-bilevel-fixtures.py
    cargo fmt --all

The `cargo fmt` is not optional and not cosmetic: this script wraps the
byte arrays at a fixed column, `rustfmt` wraps `u8` array literals at 16
elements per line, and `cargo fmt --check` is a shipping gate. The
committed output is therefore the rustfmt-normalized form, exactly as
`tools/gen-jpeg-fixtures.py`'s is.

Requires Pillow with libtiff (a developer-machine dependency only —
never a pdfcer dependency). Verified with Pillow 12.1.0.

HOW THE CCITT BYTES ARE PRODUCED, AND THE POLARITY TRAP IN DOING SO
-------------------------------------------------------------------
`hayro-ccitt` ships no encoder, and hand-writing T.4 Huffman codes from
memory is exactly the "spec-governed bytes from training-data recall"
that this project forbids. So the encoder is **libtiff**, reached through
Pillow's TIFF writer with `compression='group3'` / `'group4'`, and the
CCITT bit stream is lifted straight out of the resulting file's strip
(TIFF tags 273 StripOffsets / 279 StripByteCounts). A TIFF strip encoded
with compression 3 or 4 *is* a CCITT Group 3/4 bit stream — that is the
whole of TIFF's fax support — so no transcoding is involved.

The trap: **libtiff's fax codec is purely bit-based.** It codes runs of
0-bits as T.4 "white" runs and runs of 1-bits as T.4 "black" runs,
regardless of the PhotometricInterpretation tag. Pillow writes mode-'1'
images with Photometric = 1 (BlackIsZero) and a raw bitmap in which a
**1 bit is a white pixel**. Composing those two facts:

    T.4 "white" run  <=>  raw bit 0  <=>  Pillow BLACK pixel

and PDF's own default (`BlackIs1` false, i.e. "0 is black") makes the
decoded sample 1 wherever T.4 said white. So:

    decoded PDF sample bit  ==  NOT (Pillow raw bit)
                            ==  1 exactly where the Pillow pixel is black

Which means the Pillow source image must be built as the **visual
complement** of the picture we want the PDF to show. That is what
`_source_image` does, and it is the reason `INK` below is described in
terms of the final PDF image rather than in terms of what Pillow renders.

The derivation is not taken on trust: `fixtures_bilevel.rs` also carries
`BILEVEL_16X4_SAMPLES`, the byte-exact expected output, and the Rust
tests assert equality against it for all three CCITT variants *and* for
the JBIG2 fixture. If the polarity analysis above were wrong, every one
of those tests would fail loudly rather than the fixtures being quietly
inverted.

HOW THE JBIG2 BYTES ARE PRODUCED
--------------------------------
There is no pure-Python JBIG2 encoder, and the available test corpora are
third-party. But T.88 §6.2.6 lets a generic region be coded with **MMR**
— which is T.6, i.e. Group 4 — so the JBIG2 fixture is assembled here
segment by segment with the *same* Group 4 payload libtiff produced for
the CCITT fixture, wrapped in a hand-built embedded-stream framing
(T.88 Annex D.3: no file header, sequential segment organization).

The segment layout below is transcribed from T.88 clause 7 as
**implemented by `hayro-jbig2`'s own parser** (`src/segment.rs`,
`src/page_info.rs`, `src/decode/mod.rs`, `src/decode/generic.rs`, each of
which quotes the clause it implements). That is a deliberate choice and
it is safe for one reason: a wrong header does not produce a subtly wrong
picture, it produces a hard parse error, and the Rust test additionally
asserts the decoded samples equal `BILEVEL_16X4_SAMPLES` — the same
constant the CCITT fixtures are checked against. So the fixture is
validated twice over, structurally and semantically.

That cross-check is itself the interesting assertion: T.88 §6.2.6's
"black is 1" convention, inverted by the adapter to PDF's "0 is black",
must land on exactly the same samples as CCITT's `BlackIs1` default
reaches by a completely different route.

WHAT IT PRODUCES, AND WHY EACH ONE
----------------------------------
    BILEVEL_16X4_SAMPLES      The expected decoded samples, PDF
                              convention (0 = black), 2-byte rows.
    BILEVEL_16X4_INK          The same picture with 1 = ink, for tests
                              that assert the complement.
    CCITT_G4_16X4             K < 0. Pure two-dimensional (Group 4).
    CCITT_G3_1D_16X4          K = 0. Pure one-dimensional (Group 3 1-D),
                              the Table 11 default.
    CCITT_G3_2D_16X4          K > 0. Mixed 1-D/2-D (Group 3 2-D), via
                              TIFF's T4Options bit 0.
    JBIG2_MMR_16X4            Embedded JBIG2: page info + immediate
                              generic region, MMR coded. No globals.
    JBIG2_MMR_16X4_GLOBALS    The page information segment ALONE, as a
                              `/JBIG2Globals` stream would carry it.
    JBIG2_MMR_16X4_PAGE       The region segment alone — meaningless
                              without the globals above, which is what
                              makes the pair a real test of Table 12's
                              plumbing rather than a decoration.
"""

import io
import struct
import textwrap
from pathlib import Path

from PIL import Image

REPO = Path(__file__).resolve().parent.parent
OUT = REPO / "crates" / "pdfcer-core" / "src" / "image_codec" / "fixtures_bilevel.rs"

WIDTH = 16
HEIGHT = 4
STRIDE = (WIDTH + 7) // 8

# The picture, expressed as INK: a 1 bit is a BLACK pixel in the final
# PDF image. Chosen so that every row exercises something different:
#   row 0  a long left-hand black run then a long white run
#   row 1  the mirror image, so a 2-D coder has a maximal vertical delta
#   row 2  alternating single pixels — the T.4 worst case (NOTE 4's
#          "expansion of 2 : 9"), and the row most sensitive to an
#          off-by-one in the run bookkeeping
#   row 3  short runs at byte boundaries, which is where a stride bug
#          shows up
INK = [
    [0xFF, 0x00],
    [0x00, 0xFF],
    [0xAA, 0x55],
    [0xC3, 0xC3],
]


def _samples():
    """The expected decoded samples: PDF convention, 0 = black."""
    return bytes(0xFF ^ b for row in INK for b in row)


def _ink_bytes():
    return bytes(b for row in INK for b in row)


def _source_image():
    """Build the Pillow source: the VISUAL COMPLEMENT of the picture.

    See the module docstring's polarity derivation. Pillow mode '1'
    packs a white pixel as a 1 bit, libtiff codes 1 bits as T.4 *black*
    runs, and PDF's default makes a T.4 white run decode to sample 1. So
    a pixel we want to be ink in the PDF must be **white** here.
    """
    img = Image.new("1", (WIDTH, HEIGHT), 0)
    px = img.load()
    for y, row in enumerate(INK):
        for x in range(WIDTH):
            bit = (row[x // 8] >> (7 - (x % 8))) & 1
            px[x, y] = 255 if bit else 0
    # Sanity: the packed raw bitmap must equal INK exactly, which is the
    # premise the whole derivation rests on. If Pillow ever changes its
    # mode-'1' packing this fails here rather than silently inverting
    # every fixture.
    assert img.tobytes() == _ink_bytes(), "Pillow mode-'1' packing changed"
    return img


def _ccitt(img, **kwargs):
    """Encode with libtiff and lift the raw CCITT strip back out."""
    buf = io.BytesIO()
    img.save(buf, format="TIFF", **kwargs)
    raw = buf.getvalue()
    buf.seek(0)
    tif = Image.open(buf)
    offsets = tif.tag_v2[273]
    counts = tif.tag_v2[279]
    # A 4-row image fits in one strip at any sane RowsPerStrip, but the
    # join is written generally so a larger pattern cannot silently drop
    # data.
    return b"".join(raw[o : o + c] for o, c in zip(offsets, counts))


# ---------------------------------------------------------------------------
# JBIG2 embedded-stream assembly (T.88 clause 7, Annex D.3)
# ---------------------------------------------------------------------------

SEG_PAGE_INFORMATION = 48
SEG_IMMEDIATE_GENERIC_REGION = 38


def _segment(number, seg_type, page, data):
    """A segment header (7.2) followed by its data part.

    Header fields, in order:
      7.2.2  segment number                    u32
      7.2.3  flags: bits 0-5 type, bit 6 long page association,
             bit 7 deferred-non-retain         u8
      7.2.4  referred-to count + retain flags  u8 (short form, count 0)
      7.2.6  page association                  u8 (short form)
      7.2.7  segment data length               u32
    Only the short forms are used: this fixture has two segments, no
    cross-references, and one page.
    """
    assert 0 <= seg_type <= 0x3F
    assert 0 <= page <= 0xFF
    return (
        struct.pack(">I", number)
        + bytes([seg_type])          # short page association, retain bit clear
        + bytes([0x00])              # 0 referred-to segments, short form
        + bytes([page])
        + struct.pack(">I", len(data))
        + data
    )


def _page_information(width, height):
    """Page information segment data part (7.4.8), 19 bytes.

    width u32, height u32, X resolution u32, Y resolution u32, flags u8,
    striping u16. Resolutions are 0 = "unknown" (7.4.8.3/7.4.8.4). The
    flags byte is 0: not-eventually-lossless, no refinements, **default
    pixel 0** (a white page, so the region's black pixels are the only
    ink), default combination operator OR, no auxiliary buffers.
    Striping is 0 — the page height is known, so no EndOfStripe segments
    are needed (7.4.8.6's "page is striped" bit stays clear).
    """
    return struct.pack(">IIIIBH", width, height, 0, 0, 0x00, 0x0000)


def _generic_region(width, height, mmr_data):
    """Immediate generic region segment data part (7.4.6).

    Region segment information field (7.4.1), 17 bytes: width u32,
    height u32, X location u32, Y location u32, flags u8 (bits 0-2 the
    external combination operator, 0 = OR; bit 3 colour extension; bits
    4-7 reserved and must be 0).

    Then the generic region segment flags byte (7.4.6.2): bit 0 MMR,
    bit 1-2 GBTEMPLATE, bit 3 TPGDON, bit 4 EXTTEMPLATE. With MMR = 1 no
    adaptive-template pixels follow (they are only present for the
    arithmetic templates), so the MMR bit stream starts immediately.
    """
    info = struct.pack(">IIIIB", width, height, 0, 0, 0x00)
    flags = 0x01  # MMR = 1
    return info + bytes([flags]) + mmr_data


def build():
    img = _source_image()
    fx = {}
    fx["BILEVEL_16X4_SAMPLES"] = _samples()
    fx["BILEVEL_16X4_INK"] = _ink_bytes()
    fx["CCITT_G4_16X4"] = _ccitt(img, compression="group4")
    fx["CCITT_G3_1D_16X4"] = _ccitt(img, compression="group3")
    # TIFF tag 292 (T4Options) bit 0 = "2-dimensional coding", which is
    # what PDF calls K > 0.
    fx["CCITT_G3_2D_16X4"] = _ccitt(img, compression="group3", tiffinfo={292: 1})

    page_info = _segment(0, SEG_PAGE_INFORMATION, 1, _page_information(WIDTH, HEIGHT))
    region = _segment(
        1,
        SEG_IMMEDIATE_GENERIC_REGION,
        1,
        _generic_region(WIDTH, HEIGHT, fx["CCITT_G4_16X4"]),
    )
    fx["JBIG2_MMR_16X4"] = page_info + region
    fx["JBIG2_MMR_16X4_GLOBALS"] = page_info
    fx["JBIG2_MMR_16X4_PAGE"] = region
    return fx


DOC = {
    "BILEVEL_16X4_SAMPLES": [
        "The expected decoded samples for every fixture in this file:",
        "16 x 4, one bit per sample, **PDF convention (0 = black)**, rows",
        "padded to the 2-byte stride.",
        "",
        "Table 11's `BlackIs1` describes 1-means-black as \"the reverse of the",
        "normal PDF convention for image data\", and its default is **false** —",
        "so with no `/BlackIs1` entry the filter must emit 0 for a black pixel.",
        "With DeviceGray's default `Decode [0 1]` at 1 bit per component,",
        "sample 0 maps to grey 0.0, which is black. This constant is that rule",
        "made byte-exact.",
        "",
        "All three CCITT fixtures and the JBIG2 fixture must decode to exactly",
        "these bytes. That the two codecs agree is the point: CCITT reaches it",
        "through `/BlackIs1` false -> `invert_black` false, JBIG2 through the",
        "unconditional inverse of T.88's \"1 is black\", and the two routes have",
        "nothing in common but the answer.",
    ],
    "BILEVEL_16X4_INK": [
        "The same picture with **1 = ink**, i.e. the exact bitwise complement",
        "of [`BILEVEL_16X4_SAMPLES`].",
        "",
        "This is what `/BlackIs1 true` must produce — Table 11's \"1 bits shall",
        "be interpreted as black pixels\" — so the pair pins the polarity from",
        "both sides. A decoder with the flag wired backwards passes neither.",
    ],
    "CCITT_G4_16X4": [
        "Group 4 (pure two-dimensional, T.6) — Table 11's **K < 0**.",
        "",
        "Encoded by libtiff via Pillow (`compression='group4'`) and lifted out",
        "of the TIFF strip; see `tools/gen-bilevel-fixtures.py` for the",
        "polarity derivation that makes the source image the visual complement",
        "of the decoded picture. libtiff terminates a Group 4 strip with EOFB,",
        "so this fixture also exercises `EndOfBlock`'s default of **true**.",
        "",
        "This same byte string is reused as the MMR payload of",
        "[`JBIG2_MMR_16X4`], because T.88 §6.2.6 codes an MMR generic region",
        "with exactly T.6.",
    ],
    "CCITT_G3_1D_16X4": [
        "Group 3 one-dimensional (T.4 §4.1) — Table 11's **K = 0**, the",
        "default encoding scheme.",
        "",
        "libtiff writes an EOL pattern before each line, so this fixture also",
        "covers `EndOfLine`. Table 11 says the filter \"shall always accept",
        "end-of-line bit patterns\" whatever the flag says, and `hayro-ccitt` is",
        "unconditionally lenient about them — which is why the same bytes",
        "decode identically with `/EndOfLine` absent, true, or false.",
    ],
    "CCITT_G3_2D_16X4": [
        "Group 3 mixed one- and two-dimensional (T.4 §4.2) — Table 11's",
        "**K > 0**.",
        "",
        "Produced by setting TIFF tag 292 (T4Options) bit 0, libtiff's switch",
        "for 2-D coding. Each line carries a tag bit saying whether it was",
        "coded 1-D or 2-D, which is the structural difference from",
        "[`CCITT_G3_1D_16X4`] and the reason `K` must be trichotomous rather",
        "than boolean. Table 11 also forbids distinguishing between different",
        "positive `K` values, so this fixture must decode identically for",
        "`/K 1`, `/K 4` and `/K 40`.",
    ],
    "JBIG2_MMR_16X4": [
        "A complete **embedded** JBIG2 stream (T.88 Annex D.3): a page",
        "information segment followed by an immediate generic region segment,",
        "MMR-coded, with no `/JBIG2Globals` needed.",
        "",
        "Assembled byte by byte in `tools/gen-bilevel-fixtures.py` from T.88",
        "clause 7's segment layout, carrying [`CCITT_G4_16X4`] as its MMR",
        "payload — legal because §6.2.6 defines MMR coding as T.6. There is no",
        "pure-Python JBIG2 encoder and every available JBIG2 test corpus is",
        "third-party (`docs/LEGAL.md` §5), so assembling one is not a shortcut",
        "but the only permitted route.",
        "",
        "It must decode to [`BILEVEL_16X4_SAMPLES`] — the same bytes the CCITT",
        "fixtures produce. T.88 §6.2.6 fixes MMR black at bitmap value 1 and",
        "PDF's convention is the opposite, so the adapter's unconditional",
        "inversion is exactly what makes the two agree.",
    ],
    "JBIG2_MMR_16X4_GLOBALS": [
        "The page information segment of [`JBIG2_MMR_16X4`], **alone** — the",
        "shape a `/JBIG2Globals` stream has (Table 12: \"a stream containing the",
        "JBIG2 global segments\").",
        "",
        "Paired with [`JBIG2_MMR_16X4_PAGE`]. Neither half decodes on its own:",
        "the globals carry no region to draw, and the page carries no geometry",
        "to draw it on. That is what makes the pair a real test of Table 12's",
        "plumbing rather than a decoration — a decoder that silently ignored",
        "`/JBIG2Globals` would fail with \"missing page information\" rather than",
        "produce a wrong picture.",
    ],
    "JBIG2_MMR_16X4_PAGE": [
        "The immediate generic region segment of [`JBIG2_MMR_16X4`], **alone**.",
        "",
        "The image-stream half of the `/JBIG2Globals` pair. Decoding it without",
        "[`JBIG2_MMR_16X4_GLOBALS`] must fail cleanly (no page information",
        "segment); decoding it *with* them must produce",
        "[`BILEVEL_16X4_SAMPLES`].",
    ],
}

HEADER = '''//! # Synthetic CCITT and JBIG2 codestreams for the bilevel adapters' tests
//!
//! **GENERATED FILE — do not hand-edit.** Regenerate with:
//!
//! ```text
//! python tools/gen-bilevel-fixtures.py
//! ```
//!
//! ## Provenance (docs/LEGAL.md §5)
//!
//! Every byte array below was produced on a developer machine by
//! `tools/gen-bilevel-fixtures.py` from a **16 x 4 pixel pattern authored
//! for this project**. Nothing here was downloaded. This matters more
//! than usual for these two codecs: the obvious sources — pdf.js's and
//! PDFBox's CCITT/JBIG2 regression suites, and the public JBIG2 test
//! streams — are all third-party files of unknown provenance, which
//! `LEGAL.md` §5 forbids outright. Generating them is not a convenience,
//! it is the only permitted route.
//!
//! - **CCITT**: encoded by **libtiff** (through Pillow 12.1.0's TIFF
//!   writer, `compression='group3'`/`'group4'`) and lifted out of the
//!   resulting file's strip. A TIFF strip compressed with tag 259 = 3 or
//!   4 *is* a CCITT Group 3/4 bit stream, so no transcoding happens.
//! - **JBIG2**: assembled segment by segment from T.88 clause 7's layout,
//!   carrying the Group 4 payload above as an MMR generic region
//!   (§6.2.6 defines MMR coding as T.6). There is no pure-Python JBIG2
//!   encoder, and `hayro-jbig2` publishes no test vectors — its crate
//!   package excludes `/tests/`.
//!
//! ## The polarity derivation these fixtures rest on
//!
//! libtiff's fax codec is purely bit-based: it codes runs of 0 bits as
//! T.4 "white" and runs of 1 bits as T.4 "black", irrespective of the
//! PhotometricInterpretation tag. Pillow packs a mode-`'1'` white pixel
//! as a 1 bit. So a T.4 white run corresponds to a Pillow **black**
//! pixel, and — since `/BlackIs1` defaults to false, making a T.4 white
//! run decode to sample 1 — the decoded PDF samples are the complement
//! of Pillow's raw bitmap. The generator therefore builds the source
//! image as the *visual complement* of the picture these fixtures
//! represent.
//!
//! That derivation is asserted, not assumed: [`BILEVEL_16X4_SAMPLES`] is
//! the byte-exact expected output and every fixture here is checked
//! against it.
//!
//! ## Why byte arrays rather than files
//!
//! Hermetic tests, exactly as in the sibling `fixtures` module: embedding
//! the bytes means the tests run identically from any working directory,
//! under `cargo test`, under `cargo fuzz`, and in a `wasm32` check.

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
# Single-page demo PDFs (fixtures/synthetic/bilevel/)
# ---------------------------------------------------------------------------
#
# These are not needed by the unit tests — those embed the byte arrays
# above and stay hermetic. They exist for the two things a byte array
# cannot serve:
#
#   1. a **seed corpus** for the `image_codec_ccitt` and
#      `image_codec_jbig2` fuzz targets, which decision 005 §6.5 requires
#      to come from `fixtures/synthetic/` and never from a downloaded
#      real-world PDF (`docs/LEGAL.md` §5);
#   2. an **end-to-end demo** — `pdfcer render-page` on a real file,
#      which is the only check that exercises the whole path from xref
#      parsing through the content stream to the rasterizer.

PDF_OUT = REPO / "fixtures" / "synthetic" / "bilevel"


def _pdf(objects, root=1):
    """Assemble a classic-xref PDF from 1-based object bodies.

    `objects` is a list of `bytes`, object *n* at index *n-1*. Deliberately
    minimal (§7.5.4 cross-reference table, no object streams, no
    compression) so the file stays readable in a hex editor and a fuzzer
    mutating it keeps producing something a parser will engage with.
    """
    buf = bytearray(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n")
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


def _page_pdf(image_dict, image_data, content, extra_objects=()):
    """One 128 x 128-point page with the image drawn over the whole of it.

    `100 0 0 100 14 14 cm` maps §8.9.4's unit square onto a centred 100 pt
    square, so the picture has a visible margin and an operator can see at
    a glance which way up it landed.
    """
    objs = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 128 128] "
        b"/Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>",
        _stream("", content.encode("ascii")),
        _stream(image_dict, image_data),
    ]
    objs.extend(extra_objects)
    return _pdf(objs)


DRAW = "q 1 0 0 rg 100 0 0 100 14 14 cm /Im0 Do Q"

BILEVEL_XOBJECT = "/Type /XObject /Subtype /Image /Width 16 /Height 4"


def build_pdfs(fx):
    """The five demo/seed PDFs. Keys are file names."""
    ccitt_parms = "/DecodeParms << /K -1 /Columns 16 /Rows 4 >>"
    out = {}
    out["ccitt-g4-gray.pdf"] = _page_pdf(
        f"{BILEVEL_XOBJECT} /ColorSpace /DeviceGray /BitsPerComponent 1 "
        f"/Filter /CCITTFaxDecode {ccitt_parms}",
        fx["CCITT_G4_16X4"],
        DRAW,
    )
    out["ccitt-g4-blackis1.pdf"] = _page_pdf(
        f"{BILEVEL_XOBJECT} /ColorSpace /DeviceGray /BitsPerComponent 1 "
        f"/Filter /CCITTFaxDecode "
        f"/DecodeParms << /K -1 /Columns 16 /Rows 4 /BlackIs1 true >>",
        fx["CCITT_G4_16X4"],
        DRAW,
    )
    out["ccitt-g4-imagemask.pdf"] = _page_pdf(
        f"{BILEVEL_XOBJECT} /ImageMask true /Filter /CCITTFaxDecode {ccitt_parms}",
        fx["CCITT_G4_16X4"],
        DRAW,
    )
    out["jbig2-mmr.pdf"] = _page_pdf(
        f"{BILEVEL_XOBJECT} /ColorSpace /DeviceGray /BitsPerComponent 1 "
        f"/Filter /JBIG2Decode",
        fx["JBIG2_MMR_16X4"],
        DRAW,
    )
    # The globals variant: page information in object 6, region in the
    # image stream. This is the only one of the five that exercises
    # Table 12's reference -> stream -> filter-chain resolution.
    out["jbig2-mmr-globals.pdf"] = _page_pdf(
        f"{BILEVEL_XOBJECT} /ColorSpace /DeviceGray /BitsPerComponent 1 "
        f"/Filter /JBIG2Decode /DecodeParms << /JBIG2Globals 6 0 R >>",
        fx["JBIG2_MMR_16X4_PAGE"],
        DRAW,
        extra_objects=[_stream("", fx["JBIG2_MMR_16X4_GLOBALS"])],
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
    print(f"  expected samples: {_samples().hex()}")
    print("  NOW RUN `cargo fmt --all` — rustfmt rewraps u8 arrays at 16/line")

    PDF_OUT.mkdir(parents=True, exist_ok=True)
    for filename, data in build_pdfs(fx).items():
        (PDF_OUT / filename).write_bytes(data)
        print(f"  wrote {PDF_OUT / filename} ({len(data)} bytes)")


if __name__ == "__main__":
    main()
