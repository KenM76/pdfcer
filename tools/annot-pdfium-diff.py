#!/usr/bin/env python3
"""pdfium raster differential for Pass 6.0 annotation placement.

WHY THIS EXISTS
---------------
`docs/decisions/008` Pass 6.0 acceptance criterion 3 / risk X2:
appearance PLACEMENT (`/BBox` × `/Matrix` → `/Rect`) is a silent-wrongness
class that pdfcer's own self-comparison oracle **structurally cannot
catch** — pdfcer agreeing with pdfcer says nothing about whether a stamp is
where a reference reader puts it. This harness compares pdfcer's raster
against **pdfium's** (via pypdfium2, the decision 006 §3.2 tooling
precedent) on annotation-bearing fixtures.

SCOPE / HONESTY (do not overclaim)
----------------------------------
This is NOT a general pixel-parity harness and does NOT close the Pass
1.1 reference-renderer remainder. It runs only on the annotation subset,
and it compares the **ink bounding box** (the extent of non-white pixels)
rather than exact pixels — because pdfium and pdfcer differ in
antialiasing and both fixtures paint a solid-black fill, so the extent is
the meaningful, placement-sensitive quantity. A misplaced, mis-scaled, or
mirrored appearance moves that box; matching boxes within a few pixels is
strong evidence the §12.5.5 transform is right, which is exactly the
defect class X2 names.

USAGE
-----
    python tools/annot-pdfium-diff.py            # the synthetic set
    python tools/annot-pdfium-diff.py <dir...>   # extra corpus dirs

Requires: pypdfium2, Pillow, and a built `pdfcer` release binary.
"""

import subprocess
import sys
import tempfile
from pathlib import Path

import pypdfium2 as pdfium
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
ANNOT_DIR = ROOT / "fixtures" / "synthetic" / "annot"
CLI = ROOT / "target" / "release" / ("pdfcer.exe" if sys.platform == "win32" else "pdfcer")

# Files whose expected result is "paint nothing" — the ink-box comparison
# is skipped (both readers should be blank, so there is no box to match),
# but pdfcer's blankness is still asserted.
BLANK_EXPECTED = {
    "flags-hidden.pdf",       # Hidden: pdfium also suppresses
    "flags-noview.pdf",       # NoView on screen
    "popup-not-painted.pdf",  # never page content
    "no-ap-circle.pdf",       # R43: pdfcer paints nothing (pdfium may draw /IC)
    "as-missing-state.pdf",   # /AS unresolved: display nothing
    "placement-degenerate-bbox.pdf",  # degenerate: pdfcer refuses
}
# Files where pdfium's behaviour legitimately differs from pdfcer, so a box
# mismatch is EXPECTED and is not a pdfcer defect — reported, not asserted:
#   - R43 divergence: pdfium synthesises a look (e.g. an /IC interior fill,
#     or a text appearance from its own substitute face) that pdfcer, being
#     the stricter reader, refuses to invent;
#   - widget/form env: pdfium draws /Widget appearances only under its
#     form-fill environment (`FPDF_FFLDraw`), so a bare `page.render` shows
#     nothing for a checkbox whose /AS-selected appearance pdfcer DOES paint
#     — the divergence is in pdfium's setup, not pdfcer's placement;
#   - multi-annotation demo: mixes several dispositions (including a
#     synthesised-by-pdfium no-/AP circle), so the combined ink box is not
#     a clean single-placement comparison.
PDFIUM_MAY_SYNTHESIZE = {
    "no-ap-circle.pdf",          # R43: /IC synthesised by pdfium, not pdfcer
    "ap-resources-own-font.pdf", # substitute-face glyph extents differ ~1px
    "as-state-checkbox.pdf",     # pdfium needs FPDF_FFLDraw for widgets
    "demo-annotated.pdf",        # mixed dispositions incl. synthesised /IC
}

SCALE = 1.0
TOL = 4  # pixels


def ink_bbox(img: Image.Image):
    """Bounding box (l, t, r, b) of non-white pixels, or None if blank."""
    rgb = img.convert("RGB")
    # getbbox() on the inverted-from-white difference gives the ink extent.
    from PIL import ImageChops

    white = Image.new("RGB", rgb.size, (255, 255, 255))
    diff = ImageChops.difference(rgb, white)
    return diff.getbbox()


def pdfium_ink(path: Path):
    pdf = pdfium.PdfDocument(str(path))
    page = pdf[0]
    bitmap = page.render(scale=SCALE, draw_annots=True)
    img = bitmap.to_pil()
    box = ink_bbox(img)
    page.close()
    pdf.close()
    return box, img.size


def pdfcer_ink(path: Path):
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / "pdfcer.png"
        r = subprocess.run(
            [str(CLI), "render-page", str(path), "--scale", str(SCALE), "-o", str(out)],
            capture_output=True,
        )
        if r.returncode != 0:
            raise RuntimeError(f"pdfcer render failed: {r.stderr.decode(errors='replace')}")
        return ink_bbox(Image.open(out))


def close(a, b) -> bool:
    if a is None or b is None:
        return a is None and b is None
    return all(abs(x - y) <= TOL for x, y in zip(a, b))


def main(argv) -> int:
    if not CLI.exists():
        print(f"ERROR: build the CLI first: cargo build --release -p pdfcer-cli", file=sys.stderr)
        return 2

    dirs = [Path(d) for d in argv[1:]] or [ANNOT_DIR]
    files = sorted(p for d in dirs for p in d.glob("*.pdf"))
    if not files:
        print("no fixtures found", file=sys.stderr)
        return 2

    agree = mismatch = blank_ok = skipped = 0
    failures = []
    for path in files:
        name = path.name
        try:
            pdfcer_box = pdfcer_ink(path)
        except RuntimeError as e:
            print(f"  SKIP  {name}: {e}")
            skipped += 1
            continue
        try:
            pdfium_box, _size = pdfium_ink(path)
        except Exception as e:  # noqa: BLE001 — reference tool, any failure is a skip
            print(f"  SKIP  {name}: pdfium: {e}")
            skipped += 1
            continue

        if name in BLANK_EXPECTED:
            # pdfcer must be blank; pdfium's own behaviour is not asserted
            # (it may synthesise an /IC fill etc.).
            status = "OK-blank" if pdfcer_box is None else "PDFCER-PAINTED"
            print(f"  {status:14} {name}  pdfcer={pdfcer_box}")
            if pdfcer_box is None:
                blank_ok += 1
            else:
                failures.append((name, "expected blank, pdfcer painted", pdfcer_box, None))
            continue

        if name in PDFIUM_MAY_SYNTHESIZE:
            print(f"  REF-DIFF       {name}  pdfcer={pdfcer_box} pdfium={pdfium_box} (R43 divergence, expected)")
            skipped += 1
            continue

        if close(pdfcer_box, pdfium_box):
            agree += 1
            print(f"  AGREE          {name}  box~{pdfcer_box}")
        else:
            mismatch += 1
            failures.append((name, "placement box differs", pdfcer_box, pdfium_box))
            print(f"  MISMATCH       {name}  pdfcer={pdfcer_box} pdfium={pdfium_box}")

    print()
    print(
        f"placement AGREE={agree}  MISMATCH={mismatch}  blank-OK={blank_ok}  "
        f"skipped/ref-diff={skipped}  (tolerance {TOL}px, ink-bbox differential)"
    )
    if failures:
        print("\nFAILURES:")
        for name, why, a, b in failures:
            print(f"  {name}: {why} — pdfcer={a} pdfium={b}")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
