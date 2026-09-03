#!/usr/bin/env python3
"""Regenerate fixtures/synthetic/ocr/ - a SYNTHETIC SCAN, and the vector page it came from.

WHY THIS EXISTS
---------------
`Pass 71.0` shipped an OCR engine, a sandwich writer and the model weights, and
then recorded a warning in `ROADMAP.md` that is the reason this file exists:

    "RECOGNITION QUALITY IS UNPROVEN. Both documents available are the WRONG
     INPUT - vector PDFs that already contain text, which are
     out-of-distribution in the OPPOSITE direction from a bad scan. 62 words
     returned is a COUNT, not an accuracy result... A real quality claim needs
     a genuine scanned page and this project has no rights-cleared one."

That last clause is the whole problem. `LEGAL.md` §5 and project rule 7 forbid
checking in a real-world PDF of unknown provenance, and a real scan is exactly
that. So OCR could not be measured, and an unmeasured recogniser is
indistinguishable from a broken one.

★ THE ANSWER IS TO MANUFACTURE THE SCAN, and the reason it works is that a
scan is a LOSSY PICTURE OF A DOCUMENT THAT ALREADY EXISTED. Render a vector
page pdfcer authored, degrade the raster the way a sheet-fed scanner does, and
wrap the result as an image-only PDF. Nothing is downloaded; every byte
descends from geometry this project chose (`LEGAL.md` §5 category (a)).

★★ AND THE GROUND TRUTH COMES FREE, WHICH IS THE PART THAT MATTERS
------------------------------------------------------------------
A downloaded scan would have given a picture and no answer key - somebody would
have had to type out what it says and eyeball where. Here the vector original
IS the answer key, and it is machine-readable:

    where a word IS   =  `find-text` on `printed.pdf`  (real Std-14 font
                          metrics, through a path this project already tests)
    where OCR PUT it  =  `find-text` on the OCR'd `scan.pdf`

Two rectangles for the same word, arrived at by **completely different
routes** - one from font metrics, one from a neural recogniser looking at
pixels. They must agree. That is an equivalence the problem itself requires,
not a blessed screenshot of whatever the code did today (`R215`), and it is
the same discipline the mesh fixtures used.

★★★ IT MEASURES THE TWO FAILURES SEPARATELY, AND THEY ARE NOT THE SAME BUG
---------------------------------------------------------------------------
This is why the fixture is built this way rather than as a word-count check.

  * **A RECOGNITION failure** - the model misreads `Invoice` as `lnvoice`.
    Shows up as a word that cannot be found at all.
  * **A GEOMETRY failure** - the model reads every word perfectly and the
    invisible layer lands somewhere else: mirrored vertically (a missing
    y-flip), uniformly offset (a `/MediaBox` origin ignored), or uniformly
    scaled (a rasterisation scale that does not match the one handed to
    `words_to_page_space`).

A word count sees only the first. **The second is the one an operator
actually reports** - "the OCR text does not line up with the image" - and it
is invisible to every check that only asks *what* was read and never *where*.
A mirrored layer scores 100% on recognition.

THE FILES
---------
| file | what it is |
|---|---|
| `printed.pdf` | the vector original: known words, real Std-14 metrics. THE ANSWER KEY, not a test input |
| `scan.pdf` | `printed.pdf` rendered at 200 dpi, degraded, and wrapped as one full-page image. NO text objects at all - the OCR input |
| `scan_clean.pdf` | the same wrap with NO degradation - the control that separates "OCR is weak" from "the degradation is too harsh" |
| `scan_rotated_90.pdf` | byte-for-byte `scan.pdf` plus `/Rotate 90` - a rotation-blind mapping puts every word on the wrong axis while the page still looks perfect |
| `GROUND_TRUTH.json` | every word and its page-space rect, taken from `printed.pdf` by pdfcer itself |

★ THE DEGRADATION IS DELIBERATELY MILD, and that is a decision, not timidity.
This fixture exists to prove the PIPELINE - that a page of ordinary printed
text, scanned, comes back readable and in the right place. It is not a
stress test of the recogniser's tolerance for bad input. A fixture degraded
until recognition fails proves nothing about pdfcer and would turn every future
run into an argument about the noise level. Harder cases belong in their own
files, added deliberately, each with its own recorded expectation.

WHAT A SHEET-FED SCANNER ACTUALLY DOES, and which parts are modelled
-------------------------------------------------------------------
Modelled here, in the order applied:

  1. **Resolution loss** - rasterise at 200 dpi, the scanner default, not at
     the 600 dpi that would flatter the recogniser.
  2. **Optical blur** - a real lens and a moving sheet are not a point
     sampler. One box blur pass.
  3. **Sensor noise** - per-pixel Gaussian noise, deterministic seed.
  4. **Skew** - a sheet does not feed perfectly square. A fraction of a degree,
     which is what a good feeder gives and what every OCR engine must cope
     with.
  5. **Grey background** - paper is not #FFFFFF, and a lamp is not uniform.

NOT modelled, named so nobody assumes they were tested: JPEG artefacts (a
separate axis - `--compression jpeg` on `add-image` would add them), dust and
speckle, bleed-through from the reverse side, staple shadow, page curl.

★ THE SEED IS FIXED AND THE NOISE IS DETERMINISTIC. A fixture that changed
between regenerations would make every measurement incomparable with the one
before it, and a regression would be indistinguishable from new noise.

SOURCE MATERIAL AND LICENCE (LEGAL.md §5, project rule 7)
----------------------------------------------------------
**Nothing here derives from a third-party file.** `LEGAL.md` §5 category (a):
wholly synthetic, authored for this project, generated by this committed
script. The words are ordinary English chosen for legibility; the geometry is
this project's. Nothing was downloaded from anywhere, and no real document -
scanned or otherwise - was involved at any stage.

HOW TO REGENERATE
-----------------
    python tools/gen-ocr-fixtures.py

Requires a built `pdfcer` (release preferred) for the render and wrap steps;
the script finds it and says so if it cannot.
"""

from __future__ import annotations

import json
import math
import struct
import subprocess
import sys
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "fixtures" / "synthetic" / "ocr"

# US Letter. A scanner's own default, so the fixture does not also test an
# unusual page size.
PAGE_W, PAGE_H = 612, 792

# 200 dpi: the sheet-fed default. Deliberately NOT 300 or 600 - a fixture that
# only passes at a resolution nobody scans at proves nothing about the field.
DPI = 200
SCALE = DPI / 72.0

# ---------------------------------------------------------------------------
# The words. Chosen, not arbitrary.
# ---------------------------------------------------------------------------
# Every word is >= 4 characters and contains no digit-letter confusable pair
# (`0`/`O`, `1`/`l`/`I`, `5`/`S`) as its ONLY distinguishing feature. That is
# not to make the test easy - it is so that a failure means "the pipeline is
# broken", not "the model made the single substitution every OCR engine on
# earth makes". Confusable handling is a recogniser-quality question and
# belongs in a fixture built for it.
#
# They are laid out as a plain block of running text at 12 pt, because that is
# what a scanned page is. A page of isolated words at 40 pt would be an easier
# problem than the one this is meant to measure.
LINES = [
    "The quick brown foxes jumped over that lazy sleeping dog",
    "Recognition quality cannot be measured without a scanned page",
    "This document was never printed and never scanned by anyone",
    "Every pixel below descends from geometry this project authored",
    "Searching should find these words exactly where the eye sees them",
]

FONT_SIZE = 12
LINE_GAP = 28
TOP_Y = 700
LEFT_X = 72


def escape(s: str) -> bytes:
    return s.replace("\\", r"\\\\").replace("(", r"\(").replace(")", r"\)").encode("latin-1")


def stream_obj(body: bytes) -> bytes:
    return b"<< /Length " + str(len(body)).encode() + b" >>\nstream\n" + body + b"\nendstream"


def pdf(objects: list[bytes]) -> bytes:
    """Serialise objects 1..n with a classic §7.5.4 cross-reference table.

    No object streams and no compression, deliberately: a failing fixture must
    be readable in a hex editor, and a fuzzer mutating one keeps producing
    something a parser will engage with.
    """
    out = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
    offsets = [0]
    for i, obj in enumerate(objects, start=1):
        offsets.append(len(out))
        out += str(i).encode() + b" 0 obj\n" + obj + b"\nendobj\n"
    startxref = len(out)
    n = len(objects) + 1
    out += b"xref\n0 " + str(n).encode() + b"\n0000000000 65535 f \n"
    for off in offsets[1:]:
        out += ("%010d 00000 n \n" % off).encode()
    out += (
        b"trailer\n<< /Size " + str(n).encode() + b" /Root 1 0 R >>\n"
        b"startxref\n" + str(startxref).encode() + b"\n%%EOF\n"
    )
    return bytes(out)


def printed_pdf() -> bytes:
    """The vector original - and therefore the answer key.

    Helvetica, a Std-14 font, so the word rectangles `find-text` reports come
    from real AFM metrics through a path this project already tests. That is
    what makes the ground truth trustworthy without anybody typing it in.
    """
    parts = ["BT /F1 %d Tf\n" % FONT_SIZE]
    y = TOP_Y
    for line in LINES:
        parts.append(f"1 0 0 1 {LEFT_X} {y} Tm ({line.replace(chr(92), '')}) Tj\n")
        y -= LINE_GAP
    parts.append("ET\n")
    content = "".join(parts).encode("latin-1")

    return pdf([
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 "
        + str(PAGE_W).encode() + b" " + str(PAGE_H).encode()
        + b"] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        stream_obj(content),
    ])


# ---------------------------------------------------------------------------
# PNG read/write - deliberately hand-rolled, no image dependency
# ---------------------------------------------------------------------------
# This script must run on a bare Python with no `pip install`, because a
# fixture generator that needs a package nobody has is a fixture generator
# nobody runs, and a fixture nobody can regenerate becomes a blessed binary
# whose provenance decays into "it was there when I arrived".


def png_read_gray(data: bytes) -> tuple[int, int, bytearray]:
    """Decode a non-interlaced 8-bit PNG to one grey byte per pixel.

    Handles colour types 0 (grey), 2 (RGB) and 6 (RGBA) - which is every type
    `render-page` can emit. Anything else is refused BY NAME rather than
    producing a plausible wrong picture.
    """
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("not a PNG")
    pos, idat, w, h, depth, ctype = 8, bytearray(), 0, 0, 0, 0
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos:pos + 4])
        ctag = data[pos + 4:pos + 8]
        body = data[pos + 8:pos + 8 + length]
        if ctag == b"IHDR":
            w, h, depth, ctype = struct.unpack(">IIBB", body[:10])
        elif ctag == b"IDAT":
            idat += body
        elif ctag == b"IEND":
            break
        pos += 12 + length
    if depth != 8 or ctype not in (0, 2, 6):
        raise ValueError(f"unsupported PNG: depth={depth} colour type={ctype}")
    channels = {0: 1, 2: 3, 6: 4}[ctype]
    raw = zlib.decompress(bytes(idat))

    # §PNG 9.2 filtering, undone one scanline at a time. Written out rather
    # than pulled from a library because the whole point of this module is to
    # have no dependency.
    stride = w * channels
    out = bytearray(w * h)
    prev = bytearray(stride)
    p = 0
    for row in range(h):
        ftype = raw[p]
        p += 1
        line = bytearray(raw[p:p + stride])
        p += stride
        if ftype == 1:
            for i in range(channels, stride):
                line[i] = (line[i] + line[i - channels]) & 0xFF
        elif ftype == 2:
            for i in range(stride):
                line[i] = (line[i] + prev[i]) & 0xFF
        elif ftype == 3:
            for i in range(stride):
                left = line[i - channels] if i >= channels else 0
                line[i] = (line[i] + ((left + prev[i]) >> 1)) & 0xFF
        elif ftype == 4:
            for i in range(stride):
                a = line[i - channels] if i >= channels else 0
                b = prev[i]
                c = prev[i - channels] if i >= channels else 0
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                pred = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[i] = (line[i] + pred) & 0xFF
        elif ftype != 0:
            raise ValueError(f"bad PNG filter type {ftype}")
        prev = line
        base = row * w
        if channels == 1:
            out[base:base + w] = line
        else:
            for x in range(w):
                o = x * channels
                # Rec.601 luma. The same weights §11.5.3 uses for luminosity,
                # and the ones a greyscale scanner applies.
                out[base + x] = (line[o] * 299 + line[o + 1] * 587 + line[o + 2] * 114) // 1000
    return w, h, out


def png_write_gray(w: int, h: int, pix: bytearray) -> bytes:
    """Encode 8-bit greyscale as a non-interlaced PNG, filter type 0."""
    raw = bytearray()
    for row in range(h):
        raw.append(0)
        raw += pix[row * w:(row + 1) * w]

    def chunk(tag: bytes, body: bytes) -> bytes:
        return struct.pack(">I", len(body)) + tag + body + struct.pack(
            ">I", zlib.crc32(tag + body) & 0xFFFFFFFF
        )

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 0, 0, 0, 0))
        # pHYs so the raster declares the resolution it was made at. Not read by
        # this pipeline (`add-image --stretch` sets the size explicitly), but a
        # scan that does not say its own dpi is a scan that lies about itself.
        + chunk(b"pHYs", struct.pack(">IIB", int(DPI / 0.0254), int(DPI / 0.0254), 1))
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )


# ---------------------------------------------------------------------------
# The degradation
# ---------------------------------------------------------------------------
# A tiny deterministic PRNG rather than `random`: the fixture must be
# byte-identical on every machine and every Python version, and `random`'s
# stream is a documented-but-movable implementation detail.


class Rng:
    """xorshift64*, seeded fixed. Deterministic across platforms and versions."""

    def __init__(self, seed: int) -> None:
        self.s = seed & 0xFFFFFFFFFFFFFFFF

    def next_u64(self) -> int:
        x = self.s
        x ^= (x >> 12) & 0xFFFFFFFFFFFFFFFF
        x ^= (x << 25) & 0xFFFFFFFFFFFFFFFF
        x ^= (x >> 27) & 0xFFFFFFFFFFFFFFFF
        self.s = x & 0xFFFFFFFFFFFFFFFF
        return (self.s * 0x2545F4914F6CDD1D) & 0xFFFFFFFFFFFFFFFF

    def unit(self) -> float:
        return self.next_u64() / 2.0**64

    def gauss(self) -> float:
        """Box-Muller. Two uniforms in, one normal out."""
        u1 = max(self.unit(), 1e-12)
        u2 = self.unit()
        return math.sqrt(-2.0 * math.log(u1)) * math.cos(2.0 * math.pi * u2)


def skew(w: int, h: int, pix: bytearray, degrees: float, bg: int) -> bytearray:
    """Rotate about the page centre by a fraction of a degree.

    Nearest-neighbour on purpose. A bilinear resample would ALSO blur, and this
    step must contribute skew and nothing else - otherwise the blur below
    cannot be reasoned about or turned off independently.
    """
    rad = math.radians(degrees)
    cos_r, sin_r = math.cos(rad), math.sin(rad)
    cx, cy = w / 2.0, h / 2.0
    out = bytearray([bg]) * (w * h)
    for y in range(h):
        dy = y - cy
        for x in range(w):
            dx = x - cx
            sx = int(cx + dx * cos_r + dy * sin_r)
            sy = int(cy - dx * sin_r + dy * cos_r)
            if 0 <= sx < w and 0 <= sy < h:
                out[y * w + x] = pix[sy * w + sx]
    return out


def box_blur(w: int, h: int, pix: bytearray) -> bytearray:
    """One separable 3x1 box pass each way - a lens that is not a point sampler."""
    tmp = bytearray(w * h)
    for y in range(h):
        base = y * w
        for x in range(w):
            a = pix[base + max(x - 1, 0)]
            b = pix[base + x]
            c = pix[base + min(x + 1, w - 1)]
            tmp[base + x] = (a + b + c) // 3
    out = bytearray(w * h)
    for y in range(h):
        up = max(y - 1, 0) * w
        mid = y * w
        dn = min(y + 1, h - 1) * w
        for x in range(w):
            out[mid + x] = (tmp[up + x] + tmp[mid + x] + tmp[dn + x]) // 3
    return out


def degrade(w: int, h: int, pix: bytearray) -> bytearray:
    """Optical blur, then skew, then noise and a paper-grey lift.

    ★ THE ORDER IS PHYSICAL, not arbitrary, and getting it backwards would
    make the fixture model something that cannot happen: the lens blurs the
    PAGE (so blur precedes the sheet's skew only in that both are optical),
    and the SENSOR adds noise last, downstream of every optical effect. Noise
    applied before a blur would be smoothed by it, which is precisely what a
    real sensor's noise is not.
    """
    pix = box_blur(w, h, pix)
    pix = skew(w, h, pix, 0.35, 255)
    rng = Rng(0x5EED_0CE_A_0001)
    out = bytearray(w * h)
    for i, v in enumerate(pix):
        # Paper is not white and a scanner lamp is not uniform: compress the
        # range into [8, 246] before noise, so pure black and pure white -
        # values a real scan almost never contains - do not survive.
        base = 8 + (v * 238) // 255
        noisy = base + int(rng.gauss() * 6.0)
        out[i] = 0 if noisy < 0 else (255 if noisy > 255 else noisy)
    return out


# ---------------------------------------------------------------------------
# Driving pdfcer
# ---------------------------------------------------------------------------


def cli() -> Path:
    for rel in ("target/release/pdfcer.exe", "target/debug/pdfcer.exe",
                "target/release/pdfcer", "target/debug/pdfcer"):
        p = ROOT / rel
        if p.exists():
            return p
    sys.exit("pdfcer not built - run `cargo build --release -p pdfcer-cli` first")


def run(*args: str) -> str:
    r = subprocess.run([str(cli()), *args], capture_output=True, text=True, encoding="utf-8")
    if r.returncode != 0:
        sys.exit(f"pdfcer {' '.join(args)} failed ({r.returncode}):\n{r.stdout}\n{r.stderr}")
    return r.stdout + r.stderr


def ground_truth(printed: Path) -> dict:
    """Ask pdfcer where each word is IN THE VECTOR ORIGINAL.

    ★ This is the answer key, and it is generated rather than typed. It comes
    from `find-text`, i.e. from real Helvetica AFM metrics through the same
    extraction path `Pass 4` tests - so a word's rect here is not this
    script's opinion about where a word is, it is the document's.

    A word appearing more than once on the page would give `find-text` two
    hits and make "the" rect ambiguous, so such words are recorded with every
    hit and the comparison later requires only that OCR's rect match ONE of
    them. Silently taking the first would have quietly measured the wrong
    thing on exactly the commonest words.
    """
    words: dict[str, list[list[float]]] = {}
    seen = []
    for line in LINES:
        for wtok in line.split():
            if wtok in seen:
                continue
            seen.append(wtok)
    for wtok in seen:
        out = run("find-text", str(printed), "--needle", wtok)
        rects = []
        for ln in out.splitlines():
            if ln.startswith("match ") and "rect=" in ln:
                rect = ln.split("rect=", 1)[1].split()[0]
                rects.append([float(v) for v in rect.split(",")])
        if rects:
            words[wtok] = rects
    return {
        "page_width": PAGE_W,
        "page_height": PAGE_H,
        "dpi": DPI,
        "scale": SCALE,
        "lines": LINES,
        "words": words,
    }


def rotate_pdf(src: Path, dst: Path, degrees: int) -> None:
    """Copy a one-page PDF and set its `/Rotate`, using pdfcer's OWN verb.

    ★ WHY A ROTATED FIXTURE EXISTS AT ALL, and it is not tidiness.

    `pdfcer-render` HONOURS `/Rotate` - `page_device_geometry` swaps the raster's
    width and height at 90 and 270 and composes a different transform for each
    of Table 30's four values. Until `Pass 128.1` the OCR chain did not: it
    scaled and y-flipped and nothing else. So a caller that rasterised a
    rotated page and mapped the words back combined a rotation-AWARE rasteriser
    with a rotation-BLIND mapping.

    The result is an invisible text layer transposed relative to the ink. **The
    page still looks perfect**, because nothing visible was added, and the only
    symptom is that selecting a word gets a different one.

    ★★ SCANNED MATERIAL IS WHERE THIS BITES. Scanner drivers and every "rotate
    pages" command in every other tool write `/Rotate` rather than re-imaging
    the page, so a rotated page is not an edge case in the one population OCR
    exists for - it is the norm.

    ★★★ AND WHY THIS CALLS `rotate-page` RATHER THAN PATCHING BYTES.

    The first draft inserted `/Rotate 90` into the page dictionary and then
    rebuilt the file from its numbered objects, the way the other generators
    here do. **That silently produced a 448-byte file from a 2.3 MB input** -
    the object walk assumes the flat `N 0 obj` layout THIS SCRIPT emits, and
    `scan.pdf` is written by `add-image`, whose output it has no business
    assuming anything about. It did not raise; it just lost the image.

    Using pdfcer's own verb is better on every axis: it is the tested path, it
    performs an INCREMENTAL save so the original bytes survive verbatim
    (project rule 3), and the fixture then exercises the same `/Rotate` a real
    file would carry rather than one this script invented a representation for.
    A fixture generator that hand-rolls what the product already does is a
    second implementation, and it gets things wrong the same way.
    """
    run("rotate-page", str(src), "--page", "1", "--degrees", str(degrees),
        "--output", str(dst))


def wrap_as_scan(png: Path, out_pdf: Path) -> None:
    """Wrap a raster as an image-only PDF - a scan, structurally.

    `--stretch` and a rect that IS the full MediaBox, deliberately: a centred
    fit would introduce a margin whose size depends on rounding, and the
    ground truth is expressed in full-page coordinates. The image must map
    1:1 onto the page or every positional comparison downstream inherits an
    offset nobody put there on purpose.
    """
    blank = OUT / "_blank.pdf"
    blank.write_bytes(pdf([
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 "
        + str(PAGE_W).encode() + b" " + str(PAGE_H).encode()
        + b"] /Resources << >> /Contents 4 0 R >>",
        stream_obj(b""),
    ]))
    run("add-image", str(blank), "--image", str(png), "--page", "1",
        "--rect", f"0,0,{PAGE_W},{PAGE_H}", "--stretch", "--output", str(out_pdf))
    blank.unlink()


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)

    printed = OUT / "printed.pdf"
    printed.write_bytes(printed_pdf())
    print(f"wrote {printed}  ({printed.stat().st_size} bytes)")

    # Render the vector original at scanner resolution.
    raster = OUT / "_render.png"
    run("render-page", str(printed), "--page", "1", "--scale", f"{SCALE}",
        "--output", str(raster))
    w, h, pix = png_read_gray(raster.read_bytes())
    print(f"rasterised {w}x{h} at {DPI} dpi")

    # The CONTROL: same wrap, no degradation. Without it, a poor score cannot
    # be attributed - "the recogniser is weak" and "the degradation is too
    # harsh" produce the identical number.
    clean_png = OUT / "_clean.png"
    clean_png.write_bytes(png_write_gray(w, h, pix))
    wrap_as_scan(clean_png, OUT / "scan_clean.pdf")
    print(f"wrote {OUT / 'scan_clean.pdf'}  ({(OUT / 'scan_clean.pdf').stat().st_size} bytes)")

    # The scan proper.
    scan_png = OUT / "_scan.png"
    scan_png.write_bytes(png_write_gray(w, h, degrade(w, h, pix)))
    wrap_as_scan(scan_png, OUT / "scan.pdf")
    print(f"wrote {OUT / 'scan.pdf'}  ({(OUT / 'scan.pdf').stat().st_size} bytes)")

    # The rotated sibling: the SAME scan, one dictionary entry different.
    rotate_pdf(OUT / "scan.pdf", OUT / "scan_rotated_90.pdf", 90)
    print(f"wrote {OUT / 'scan_rotated_90.pdf'}  "
          f"({(OUT / 'scan_rotated_90.pdf').stat().st_size} bytes)")

    truth = ground_truth(printed)
    (OUT / "GROUND_TRUTH.json").write_text(json.dumps(truth, indent=2), encoding="utf-8")
    print(f"wrote {OUT / 'GROUND_TRUTH.json'}  ({len(truth['words'])} distinct words)")

    for tmp in (raster, clean_png, scan_png):
        tmp.unlink(missing_ok=True)

    # A scan that still contains text objects is not a scan, and every
    # measurement built on it would be meaningless - OCR would appear to
    # "work" while `find-text` was reading the original text layer straight
    # through. Checked here rather than trusted, because the failure is
    # invisible: the numbers all come out RIGHT.
    for name in ("scan.pdf", "scan_clean.pdf", "scan_rotated_90.pdf"):
        out = run("extract-text", str(OUT / name))
        codes = [t for t in out.split() if t.startswith("codes=")]
        assert codes and codes[0] == "codes=0", f"{name} still contains text: {codes}"
    print("verified: both scans contain ZERO text objects")


if __name__ == "__main__":
    main()
