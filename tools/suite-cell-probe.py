#!/usr/bin/env python3
"""Probe a suite trap cell: what pdfcer painted, and what Acrobat painted there.

WHY THIS EXISTS
---------------
`tools/suite-check.py` answers "did a trap X fire?" — a pass/fail verdict that
matches the suite's own criterion. It deliberately says nothing about *why*,
and for a failing patch that is the entire remaining question.

The suite trap X is drawn so that a CORRECT engine renders the X **the same
colour as the swatch it sits on**, making it invisible. That design gives a
free oracle that costs no reference render at all:

  * the SURROUND is the plainly-painted swatch — usually the simpler object;
  * the X is the object carrying the feature under test (a blend mode, an
    overprint state, a soft mask);
  * so when they differ, the difference localises to ONE of the two, and the
    numbers say which.

Add a known-good render at the same cell and the triple becomes decisive:

  * pdfcer surround ~= Acrobat, pdfcer X far from both
        -> the FEATURE object is wrong; the plain paint path is fine.
  * pdfcer X ~= a saturated primary
        -> the blend produced `cs` unchanged, i.e. it was composited against
           NOTHING. That is the signature of a transparent-initialised group
           buffer standing in for a non-isolated group.
  * both differ from Acrobat
        -> a colour-conversion difference, not a compositing one.

This tool turned "14 traps on PCS1_161" into "every interior blend is being
applied against a transparent backdrop" in a single run. See
`docs/compositor-plan.md` §1 for the diagnosis it produced.

INPUTS
------
  patch          the patch stem, e.g. `PCS1_160_Transp_Basic_BM_DeviceCMYK_Non-knockout_X4`
  --render-dir   where suite-check.py left `<patch>.pdf.png`  (default: alongside the corpus)
  --reference-dir  known-good renders named `<patch>.png`     (optional but this is the point)

The corpus and the reference renders live OUTSIDE the repository — test-corpus
rules, `docs/LEGAL.md` §5. Nothing here writes into the repo.

OUTPUT
------
One line per trap:

    cell @( 204, 106) 38x38  X=[178 178 178]  surround=[20 20 20]  reference=[24 23 22]

WHAT IT DELIBERATELY DOES NOT DO
--------------------------------
It does not name the blend mode of a cell. Doing that reliably means reading
the patch's content stream, resolving the `/ExtGState` each `Do` selects, and
mapping the form XObject's placement matrix into device space — real work, and
work that would be wrong in a quiet way if any step were approximated. The
manual derivation for `PCS1_160` is written out in `docs/compositor-plan.md`
§1.1 (cell pitch 31.68 pt, 22.678 pt squares, render scale 2.0) so the
arithmetic is at least reproducible. Promoting that derivation into this file
is a genuine improvement and is listed as owed.

EXIT CODES
----------
  0  probed successfully (including "no traps found" — that is a real answer)
  2  a required render was missing
"""

import argparse
import importlib.util
import os
import sys

import cv2
import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))


def _load_suite_check():
    """Import `suite-check.py` for `find_traps`.

    The hyphen in the filename makes it not a legal module name, so a normal
    import cannot reach it. Reusing its detector rather than re-implementing
    one is the point: a second trap-finder with slightly different thresholds
    would report a different set of cells than the harness does, and then the
    two tools would disagree about which cells are even failing.
    """
    path = os.path.join(HERE, "suite-check.py")
    spec = importlib.util.spec_from_file_location("suite_check", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def _dominant_split(band_gray, fill, w, h):
    """Separate the X's pixels from the surround's inside a trap bounding box.

    `find_traps` segments on EXACT intensity level, so both the X and its
    surround are single flat values in the greyscale image. Recover the X's
    level by picking the level whose pixel count is closest to the area the
    detector already measured (`fill * w * h`). That reuses the detector's own
    measurement instead of re-thresholding, so the two can never disagree
    about which pixels are the X.
    """
    levels, counts = np.unique(band_gray, return_counts=True)
    target = fill * w * h
    x_level = levels[int(np.argmin(np.abs(counts - target)))]
    return band_gray == x_level


def probe(render_png, reference_png=None):
    gc = _load_suite_check()
    rgb = cv2.imread(render_png, cv2.IMREAD_COLOR)
    if rgb is None:
        return None
    rgb = rgb[:, :, ::-1].astype(float)          # BGR -> RGB
    gray = cv2.imread(render_png, cv2.IMREAD_GRAYSCALE)

    ref = None
    if reference_png and os.path.exists(reference_png):
        ref = cv2.imread(reference_png, cv2.IMREAD_COLOR)
        if ref is not None:
            ref = ref[:, :, ::-1].astype(float)

    rows = []
    for (x, y, w, h, fill, _diag) in gc.find_traps(render_png):
        band_gray = gray[y:y + h, x:x + w]
        band_rgb = rgb[y:y + h, x:x + w]
        m = _dominant_split(band_gray, fill, w, h)
        inside = band_rgb[m].mean(axis=0)
        outside = band_rgb[~m].mean(axis=0)

        ref_rgb = None
        if ref is not None:
            # The reference render is at whatever scale the reference engine
            # produced; rescale the CENTRE of the cell rather than the image,
            # because resampling a flat swatch to compare flat swatches only
            # adds interpolation error at the edges.
            sy = ref.shape[0] / rgb.shape[0]
            sx = ref.shape[1] / rgb.shape[1]
            cy, cx = int((y + h / 2) * sy), int((x + w / 2) * sx)
            r = max(int(w * sx * 0.3), 1)
            win = ref[max(cy - r, 0):cy + r, max(cx - r, 0):cx + r]
            if win.size:
                ref_rgb = win.reshape(-1, 3).mean(axis=0)

        rows.append((x, y, w, h, inside, outside, ref_rgb))
    return rows


def _default_render_dir():
    """`$PDFCER_SUITE_DIR/_render`, or None when the corpus is not configured.

    The licensed suite is not named in this repository and neither is its
    location on disk (operator ruling 2026-08-25), so there is no hard-coded
    fallback to reach for. Returning None makes the caller pass `--render-dir`
    explicitly rather than silently probing a directory that does not exist on
    a fresh clone.
    """
    root = os.environ.get("PDFCER_SUITE_DIR")
    return os.path.join(root, "_render") if root else None


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("patch", help="patch stem, without .pdf or .png")
    ap.add_argument(
        "--render-dir",
        default=_default_render_dir(),
        help="where suite-check.py left <id>.pdf.png; defaults to "
        "$PDFCER_SUITE_DIR/_render, and must be given explicitly when that "
        "variable is unset, because no corpus path may be hard-coded in "
        "this repository (operator ruling 2026-08-25)",
    )
    ap.add_argument("--reference-dir", default=None)
    args = ap.parse_args()

    render = os.path.join(args.render_dir, args.patch + ".pdf.png")
    if not os.path.exists(render):
        print(f"suite-cell-probe: no render at {render}\n"
              f"  run tools/suite-check.py first -- it is what produces them.",
              file=sys.stderr)
        return 2
    reference = (os.path.join(args.reference_dir, args.patch + ".png")
                 if args.reference_dir else None)

    rows = probe(render, reference)
    if rows is None:
        print(f"suite-cell-probe: could not read {render}", file=sys.stderr)
        return 2
    if not rows:
        # ASCII only in printed output: this runs on a Windows console under
        # cp1252, where an em dash renders as a replacement glyph and makes a
        # correct answer look like a mojibake bug.
        print("no traps found -- this patch is clean, or it is a "
              "reference-strip patch that carries no X")
        return 0

    for (x, y, w, h, inside, outside, ref_rgb) in rows:
        ref_txt = "" if ref_rgb is None else f"  reference={np.round(ref_rgb).astype(int)}"
        print(f"cell @({x:4d},{y:4d}) {w}x{h}  "
              f"X={np.round(inside).astype(int)}  "
              f"surround={np.round(outside).astype(int)}{ref_txt}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
