#!/usr/bin/env python3
"""Generate the TIFF fixtures for `pdfcer_core::image_import::tiff`.

WHY THIS EXISTS
---------------
`pdfcer-core`'s TIFF reader turns a baseline TIFF 6.0 file into a PDF image
XObject.  Proving it needs *real container bytes*: a hand-written unit test
over a synthetic 20-byte "TIFF" proves the directory walker and nothing
else.  Only an actual LZW-compressed, horizontally-differenced, multi-strip
directory proves that pdfcer reassembles strips in order, undoes TIFF 6.0
§14's predictor per channel, and byte-swaps 16-bit samples out of the file's
byte order into ISO 32000-1 §8.9.3's.

Those bytes cannot be downloaded.  `docs/LEGAL.md` §5 and project rule 7
permit only synthetic or clearly rights-cleared test data, and a TIFF pulled
off the web is neither.  They are therefore GENERATED here, from the SAME
authored pixel data `tools/gen-image-fixtures.py` uses for the PNG/JPEG/BMP
fixtures — flat colour ramps, an alpha ramp and a six-entry palette, nothing
derived from any third-party work — and written next to this script.

Everything is written byte by byte with the standard library (`struct`,
`zlib`, plus an LZW and a PackBits encoder implemented below).  Pillow is
deliberately NOT used, for two reasons that both matter:

  1.  Pillow drives libtiff, so a Pillow-produced fixture would pin
      libtiff's choices about strip size, predictor use, tag order and
      compression level rather than choices this project made.
  2.  `rgb8-deflate.tif` pins pdfcer's *passthrough* branch — the assertion
      is that the embedded PDF stream is the TIFF strip's own bytes, byte
      for byte.  Those bytes have to be bytes this project chose.

This script is NOT part of the build.  It is run by hand when the fixture
set needs to change, and its output is committed.  It is not a Cargo
workspace member, so it never enters the dependency graph or
THIRD_PARTY_LICENSES.md.

USAGE
-----
    python fixtures/synthetic/tiff/gen-tiff-fixtures.py

(It writes into its own directory, so it can be run from anywhere.)

NOTE ON LOCATION
----------------
Every other fixture generator in this project lives in `tools/` and is named
`gen-*-fixtures.py`.  This one sits beside its output instead, because the
session that authored it was scoped to `fixtures/synthetic/` and could not
create files under `tools/`.  Moving it to `tools/gen-tiff-fixtures.py` (and
updating the path in `PROVENANCE.md` and in
`crates/pdfcer-core/tests/image_tiff.rs`) is the right follow-up; nothing
depends on where it lives.

WHAT EACH FIXTURE PINS
----------------------
Byte order — the single most common TIFF bug:
    gray8-be.tif / gray8-le.tif        the same picture in both byte orders;
                                       must import to identical samples.
    gray16-be.tif / gray16-le.tif      the same at 16 bits, where the SAMPLE
                                       DATA is also byte-order-dependent.
                                       An implementation that swaps the tags
                                       but not the samples passes the 8-bit
                                       pair and fails this one.

Compression (all five pdfcer accepts decode to the same samples):
    rgb8-none.tif                      Compression 1, one strip.
    rgb8-lzw.tif                       Compression 5 + Predictor 2, three
                                       strips — the shape a real tool writes.
    rgb8-deflate.tif                   Compression 8, ONE strip, Predictor 1:
                                       the narrow case whose strip payload is
                                       already a legal /FlateDecode stream and
                                       is passed through verbatim.
    rgb8-deflate-strips.tif            Compression 32946, three strips: the
                                       same picture, but several independent
                                       zlib streams, so NOT passthrough.
    rgb8-packbits.tif                  Compression 32773, and the strip carries
                                       a deliberate 0x80 byte — a no-op in
                                       TIFF 6.0 §9 and END-OF-DATA in
                                       ISO 32000-1 §7.4.5.  A reader that
                                       reuses the PDF RunLengthDecode filter
                                       loses everything after it.

Photometric interpretation:
    bilevel-whiteiszero.tif            PhotometricInterpretation 0 at 1 bit —
                                       the fax/CAD default.  Read without the
                                       complement, the picture is a negative.
    gray8-be.tif                       PhotometricInterpretation 1, the
                                       control.
    pal8.tif                           Palette with a spec-conformant 16-bit
                                       ColorMap stored as three consecutive
                                       BLOCKS (TIFF 6.0 §16), not triples.
    pal8-8bit-colormap.tif             The same palette written 0..255 in
                                       those 16-bit fields — the real-world
                                       divergence libtiff's heuristic exists
                                       for.  pdfcer detects it and DISCLOSES.

Alpha (ExtraSamples, TIFF 6.0 §18):
    rgba8-unassociated.tif             ExtraSamples 2 — straight alpha, splits
                                       directly into base + /SMask.
    rgba8-associated.tif               ExtraSamples 1 — colour PREMULTIPLIED
                                       by alpha.  Must be un-premultiplied for
                                       a straight-alpha /SMask, or every
                                       partly-transparent pixel composites
                                       double-dark.  The alpha ramp is
                                       deliberately non-binary so the two
                                       cases are distinguishable at all.
    rgba8-unspecified.tif              ExtraSamples 0 — "unspecified data".
                                       Dropped, never read as opacity.

Multi-page:
    multipage.tif                      Three IFDs.  pdfcer places the first and
                                       COUNTS the rest.

Refusals (each must fail BY NAME with a stable key — R27):
    tiled.tif                          TIFF/tiled
    planar.tif                         TIFF/planar-separate
    ccittg4.tif                        TIFF/ccitt-g4
    float32.tif                        TIFF/sample-format-float
    bigtiff.tif                        BigTIFF — refused at the SNIFFER, as
                                       its own format name, because it is a
                                       different parser rather than a
                                       sub-feature of classic TIFF.
"""

import struct
import zlib
from pathlib import Path

OUT = Path(__file__).resolve().parent

# ---------------------------------------------------------------------------
# The shared pixel data — identical in intent to tools/gen-image-fixtures.py
# ---------------------------------------------------------------------------

W, H = 6, 4


def rgb_rows(depth: int = 8) -> list[bytes]:
    """A red→blue horizontal ramp with a green vertical component."""
    rows = []
    for y in range(H):
        row = bytearray()
        for x in range(W):
            r = 255 * x // (W - 1)
            g = 255 * y // (H - 1)
            b = 255 - r
            if depth == 8:
                row += bytes([r, g, b])
            else:
                row += struct.pack(">HHH", r * 257, g * 257, b * 257)
        rows.append(bytes(row))
    return rows


def gray_values() -> list[list[int]]:
    return [[(255 * (x + y)) // (W + H - 2) for x in range(W)] for y in range(H)]


def alpha_at(x: int, y: int) -> int:
    """A deliberately NON-binary alpha ramp.

    Binary (0/255) alpha would round-trip through a colour-key /Mask as well
    as through an /SMask, and — the point here — would make the associated
    and unassociated cases produce identical bytes, so a fixture using it
    could not tell premultiplied from straight alpha at all.
    """
    return (255 * (x * H + y)) // (W * H - 1)


PALETTE = [
    (0, 0, 0),        # 0 black
    (255, 0, 0),      # 1 red
    (0, 255, 0),      # 2 green
    (0, 0, 255),      # 3 blue
    (255, 255, 0),    # 4 yellow
    (255, 255, 255),  # 5 white
]


def index_at(x: int, y: int) -> int:
    return (x + 2 * y) % 6


# ---------------------------------------------------------------------------
# TIFF writer (TIFF 6.0 §2), written by hand so the fixtures pin OUR bytes
# ---------------------------------------------------------------------------

# Field types.
BYTE, ASCII, SHORT, LONG, RATIONAL = 1, 2, 3, 4, 5
UNDEFINED = 7

# Tags this generator writes.
IMAGE_WIDTH = 256
IMAGE_LENGTH = 257
BITS_PER_SAMPLE = 258
COMPRESSION = 259
PHOTOMETRIC = 262
STRIP_OFFSETS = 273
ORIENTATION = 274
SAMPLES_PER_PIXEL = 277
ROWS_PER_STRIP = 278
STRIP_BYTE_COUNTS = 279
X_RESOLUTION = 282
Y_RESOLUTION = 283
PLANAR_CONFIG = 284
RESOLUTION_UNIT = 296
PREDICTOR = 317
COLOR_MAP = 320
TILE_WIDTH = 322
TILE_LENGTH = 323
EXTRA_SAMPLES = 338
SAMPLE_FORMAT = 339

TYPE_SIZE = {BYTE: 1, ASCII: 1, SHORT: 2, LONG: 4, RATIONAL: 8, UNDEFINED: 1}


def write_tiff(path: str, order: str, pages: list[tuple[list, list[bytes]]]) -> None:
    """Write a (possibly multi-page) classic TIFF.

    `order` is "II" or "MM".  Each page is `(fields, strips)` where `fields`
    is a list of `(tag, type, [values])` and `strips` are the ALREADY
    COMPRESSED strip payloads; StripOffsets/StripByteCounts are computed here
    and appended, so a fixture never has to hand-count an offset.

    Layout: header, then every page's strip data, then the IFDs with their
    overflow areas.  TIFF 6.0 §2 requires the directory entries to be sorted
    ascending by tag and the IFD itself to begin on a word boundary; both are
    done here rather than left to each fixture.
    """
    le = order == "II"
    e = "<" if le else ">"

    out = bytearray(order.encode())
    out += struct.pack(e + "H", 42)
    first_ifd_slot = len(out)
    out += struct.pack(e + "I", 0)

    # Strip payloads first, so their offsets are known before any IFD.
    spans: list[list[tuple[int, int]]] = []
    for _, strips in pages:
        page_spans = []
        for s in strips:
            page_spans.append((len(out), len(s)))
            out += s
        spans.append(page_spans)

    prev_next_slot = None
    for page_index, (fields, _) in enumerate(pages):
        page_spans = spans[page_index]
        all_fields = list(fields)
        all_fields.append((STRIP_OFFSETS, LONG, [o for o, _ in page_spans]))
        all_fields.append((STRIP_BYTE_COUNTS, LONG, [n for _, n in page_spans]))
        all_fields.sort(key=lambda f: f[0])

        if len(out) % 2:
            out += b"\x00"
        ifd_at = len(out)
        if page_index == 0:
            out[first_ifd_slot:first_ifd_slot + 4] = struct.pack(e + "I", ifd_at)
        else:
            out[prev_next_slot:prev_next_slot + 4] = struct.pack(e + "I", ifd_at)

        table = bytearray(struct.pack(e + "H", len(all_fields)))
        overflow = bytearray()
        overflow_base = ifd_at + 2 + len(all_fields) * 12 + 4

        for tag, kind, values in all_fields:
            table += struct.pack(e + "HHI", tag, kind, len(values))
            size = TYPE_SIZE[kind]
            payload = bytearray()
            for v in values:
                if kind in (BYTE, ASCII, UNDEFINED):
                    payload += struct.pack("B", v & 0xFF)
                elif kind == SHORT:
                    payload += struct.pack(e + "H", v)
                elif kind == LONG:
                    payload += struct.pack(e + "I", v)
                else:  # RATIONAL: numerator, denominator
                    payload += struct.pack(e + "II", v, 1)
            assert len(payload) == len(values) * size
            if len(payload) <= 4:
                payload += b"\x00" * (4 - len(payload))
                table += payload
            else:
                table += struct.pack(e + "I", overflow_base + len(overflow))
                overflow += payload

        prev_next_slot = ifd_at + len(table)
        table += struct.pack(e + "I", 0)
        out += table
        out += overflow

    (OUT / path).write_bytes(bytes(out))
    print(f"  {path}  ({len(out)} bytes)")


# ---------------------------------------------------------------------------
# Compressors
# ---------------------------------------------------------------------------


def deflate(data: bytes) -> bytes:
    """RFC 1950 zlib — TIFF compressions 8 and 32946, and exactly what
    ISO 32000-1 §7.4.4.1 defines /FlateDecode as."""
    return zlib.compress(data, 6)


def packbits(data: bytes, insert_noop_at: int | None = None) -> bytes:
    """PackBits (TIFF 6.0 §9), literal runs only.

    Literal runs only, because the point of these fixtures is the FRAMING,
    not the compression ratio.  `insert_noop_at` splices a bare 0x80 byte
    between two runs: TIFF 6.0 §9 says "the -128 value is not used; it is a
    no-op", while ISO 32000-1 §7.4.5 gives 128 the meaning END-OF-DATA.  A
    TIFF reader that reuses the PDF RunLengthDecode filter stops there and
    silently loses the rest of the strip.
    """
    out = bytearray()
    runs = [data[i:i + 64] for i in range(0, len(data), 64)]
    for i, chunk in enumerate(runs):
        if insert_noop_at is not None and i == insert_noop_at:
            out += b"\x80"
        out += bytes([len(chunk) - 1])
        out += chunk
    return bytes(out)


def lzw(data: bytes) -> bytes:
    """TIFF 6.0 §13 LZW: 8-bit alphabet, MSB-first packing, code widths that
    grow ONE CODE EARLY.

    The same codec ISO 32000-1 §7.4.4.2 describes with `/EarlyChange 1`
    (its default), which is why pdfcer decodes it through the shared
    `filters::lzw` with no parameters at all.
    """
    CLEAR, EOI = 256, 257
    table: dict[bytes, int] = {bytes([i]): i for i in range(256)}
    next_code = 258
    width = 9

    bits: list[int] = []

    def emit(code: int) -> None:
        for i in range(width - 1, -1, -1):
            bits.append((code >> i) & 1)

    emit(CLEAR)
    omega = b""
    for ch in data:
        k = bytes([ch])
        if omega + k in table:
            omega = omega + k
        else:
            emit(table[omega])
            table[omega + k] = next_code
            next_code += 1
            # "One code early": switch as soon as the NEXT code to be
            # assigned would not fit in the current width minus one.
            if next_code + 1 > (1 << width) and width < 12:
                width += 1
            if next_code >= 4094:
                emit(CLEAR)
                table = {bytes([i]): i for i in range(256)}
                next_code = 258
                width = 9
            omega = k
    if omega:
        emit(table[omega])
    emit(EOI)

    while len(bits) % 8:
        bits.append(0)
    out = bytearray()
    for i in range(0, len(bits), 8):
        byte = 0
        for b in bits[i:i + 8]:
            byte = (byte << 1) | b
        out.append(byte)
    return bytes(out)


def horizontal_difference(rows: list[bytes], samples: int) -> list[bytes]:
    """TIFF 6.0 §14 Predictor 2, encode direction: each 8-bit sample becomes
    the difference from the same channel of the pixel to its left."""
    out = []
    for row in rows:
        diffed = bytearray(row)
        for i in range(len(row) - 1, samples - 1, -1):
            diffed[i] = (row[i] - row[i - samples]) & 0xFF
        out.append(bytes(diffed))
    return out


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


def base_fields(width, height, bits_list, photometric, compression, rows_per_strip):
    return [
        (IMAGE_WIDTH, LONG, [width]),
        (IMAGE_LENGTH, LONG, [height]),
        (BITS_PER_SAMPLE, SHORT, bits_list),
        (COMPRESSION, SHORT, [compression]),
        (PHOTOMETRIC, SHORT, [photometric]),
        (SAMPLES_PER_PIXEL, SHORT, [len(bits_list)]),
        (ROWS_PER_STRIP, LONG, [rows_per_strip]),
        (PLANAR_CONFIG, SHORT, [1]),
        (X_RESOLUTION, RATIONAL, [300]),
        (Y_RESOLUTION, RATIONAL, [300]),
        (RESOLUTION_UNIT, SHORT, [2]),
    ]


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    print(f"writing TIFF fixtures to {OUT}")

    # --- byte order, 8 bit ------------------------------------------------
    gray = gray_values()
    gray_rows = [bytes(r) for r in gray]
    for name, order in (("gray8-be.tif", "MM"), ("gray8-le.tif", "II")):
        write_tiff(
            name,
            order,
            [(base_fields(W, H, [8], 1, 1, H), [b"".join(gray_rows)])],
        )

    # --- byte order, 16 bit: the sample data is order-dependent too -------
    for name, order, endian in (
        ("gray16-be.tif", "MM", ">"),
        ("gray16-le.tif", "II", "<"),
    ):
        payload = b"".join(
            struct.pack(endian + "H", v * 257) for row in gray for v in row
        )
        write_tiff(
            name,
            order,
            [(base_fields(W, H, [16], 1, 1, H), [payload])],
        )

    # --- compression: five encodings of ONE picture -----------------------
    rgb = rgb_rows()
    rgb_flat = b"".join(rgb)

    write_tiff(
        "rgb8-none.tif",
        "MM",
        [(base_fields(W, H, [8, 8, 8], 2, 1, H), [rgb_flat])],
    )

    # LZW + Predictor 2, three strips — what a real tool writes.
    lzw_fields = base_fields(W, H, [8, 8, 8], 2, 5, 2)
    lzw_fields.append((PREDICTOR, SHORT, [2]))
    diffed = horizontal_difference(rgb, 3)
    write_tiff(
        "rgb8-lzw.tif",
        "II",
        [(lzw_fields, [lzw(b"".join(diffed[i:i + 2])) for i in (0, 2)])],
    )

    # ONE strip, Deflate, Predictor 1 — the passthrough case.  Its embedded
    # PDF stream must be these exact bytes.
    write_tiff(
        "rgb8-deflate.tif",
        "MM",
        [(base_fields(W, H, [8, 8, 8], 2, 8, H), [deflate(rgb_flat)])],
    )

    # The same picture in three independently-deflated strips: several zlib
    # streams cannot be one PDF stream, so the passthrough must NOT fire.
    write_tiff(
        "rgb8-deflate-strips.tif",
        "II",
        [
            (
                base_fields(W, H, [8, 8, 8], 2, 32946, 2),
                [deflate(b"".join(rgb[i:i + 2])) for i in (0, 2)],
            )
        ],
    )

    # PackBits, with the 0x80 no-op spliced in.
    write_tiff(
        "rgb8-packbits.tif",
        "MM",
        [(base_fields(W, H, [8, 8, 8], 2, 32773, H), [packbits(rgb_flat, 1)])],
    )

    # --- photometric ------------------------------------------------------
    # 1-bit WhiteIsZero: 0 is imaged as WHITE, the reverse of /DeviceGray.
    # A diagonal, so a vertical or horizontal flip would also be visible.
    bilevel = bytearray()
    for y in range(H):
        byte = 0
        for x in range(W):
            bit = 1 if (x + y) % 3 == 0 else 0
            byte |= bit << (7 - x)
        bilevel.append(byte)
    write_tiff(
        "bilevel-whiteiszero.tif",
        "MM",
        [(base_fields(W, H, [1], 0, 1, H), [bytes(bilevel)])],
    )

    # Palette.  TIFF 6.0 §16 stores the ColorMap as three consecutive BLOCKS
    # (all red, then all green, then all blue), NOT as interleaved triples.
    indices = bytes(index_at(x, y) for y in range(H) for x in range(W))
    entries = 256  # 2 ** BitsPerSample
    reds = [0] * entries
    greens = [0] * entries
    blues = [0] * entries
    for i, (r, g, b) in enumerate(PALETTE):
        reds[i], greens[i], blues[i] = r * 257, g * 257, b * 257
    write_tiff(
        "pal8.tif",
        "II",
        [
            (
                base_fields(W, H, [8], 3, 1, H)
                + [(COLOR_MAP, SHORT, reds + greens + blues)],
                [indices],
            )
        ],
    )

    # The same palette written 0..255 in those 16-bit fields — the real-world
    # divergence.  Trusting TIFF 6.0 §16 literally renders it near-black.
    reds8 = [v // 257 for v in reds]
    greens8 = [v // 257 for v in greens]
    blues8 = [v // 257 for v in blues]
    write_tiff(
        "pal8-8bit-colormap.tif",
        "II",
        [
            (
                base_fields(W, H, [8], 3, 1, H)
                + [(COLOR_MAP, SHORT, reds8 + greens8 + blues8)],
                [indices],
            )
        ],
    )

    # --- alpha (ExtraSamples, TIFF 6.0 §18) -------------------------------
    straight = bytearray()
    premultiplied = bytearray()
    for y in range(H):
        for x in range(W):
            r = 255 * x // (W - 1)
            g = 255 * y // (H - 1)
            b = 255 - r
            a = alpha_at(x, y)
            straight += bytes([r, g, b, a])
            premultiplied += bytes(
                [(r * a + 127) // 255, (g * a + 127) // 255, (b * a + 127) // 255, a]
            )

    for name, payload, extra in (
        ("rgba8-unassociated.tif", bytes(straight), 2),
        ("rgba8-associated.tif", bytes(premultiplied), 1),
        ("rgba8-unspecified.tif", bytes(straight), 0),
    ):
        write_tiff(
            name,
            "II",
            [
                (
                    base_fields(W, H, [8, 8, 8, 8], 2, 1, H)
                    + [(EXTRA_SAMPLES, SHORT, [extra])],
                    [payload],
                )
            ],
        )

    # --- multi-page -------------------------------------------------------
    # Three visibly different pages, so "the FIRST page" is a real assertion
    # rather than a coincidence.
    pages = []
    for shade in (0x11, 0x22, 0x33):
        pages.append(
            (base_fields(W, H, [8], 1, 1, H), [bytes([shade]) * (W * H)])
        )
    write_tiff("multipage.tif", "MM", pages)

    # --- refusals ---------------------------------------------------------
    write_tiff(
        "tiled.tif",
        "II",
        [
            (
                base_fields(W, H, [8], 1, 1, H)
                + [(TILE_WIDTH, LONG, [16]), (TILE_LENGTH, LONG, [16])],
                [bytes(W * H)],
            )
        ],
    )
    planar = base_fields(W, H, [8, 8, 8], 2, 1, H)
    planar = [f for f in planar if f[0] != PLANAR_CONFIG]
    planar.append((PLANAR_CONFIG, SHORT, [2]))
    write_tiff("planar.tif", "II", [(planar, [bytes(W * H * 3)])])

    # Compression 4 = CCITT Group 4.  The strip content is irrelevant: pdfcer
    # must refuse at the DIRECTORY, before any decoder sees a byte — which is
    # exactly the property under test.
    write_tiff(
        "ccittg4.tif",
        "MM",
        [(base_fields(W, H, [1], 0, 4, H), [b"\x00\x01\x02\x03"])],
    )

    write_tiff(
        "float32.tif",
        "II",
        [
            (
                base_fields(W, H, [32], 1, 1, H) + [(SAMPLE_FORMAT, SHORT, [3])],
                [bytes(W * H * 4)],
            )
        ],
    )

    # BigTIFF: only the version magic matters here — pdfcer refuses it at the
    # SNIFFER, under its own format name, because it is a different parser.
    big = bytearray(b"II")
    big += struct.pack("<H", 43)
    big += struct.pack("<H", 8)   # offset size
    big += struct.pack("<H", 0)   # reserved
    big += struct.pack("<Q", 16)  # first IFD
    big += b"\x00" * 16
    (OUT / "bigtiff.tif").write_bytes(bytes(big))
    print(f"  bigtiff.tif  ({len(big)} bytes)")

    print("done")


if __name__ == "__main__":
    main()
