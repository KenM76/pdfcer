#!/usr/bin/env python3
"""flat-color-parity — how close is pdfcer's COLOUR to the reference engine's,
on flat regions, with structure taken out of the question.

WHY THIS EXISTS, AND WHY THE THREE INSTRUMENTS ALREADY HERE CANNOT ANSWER IT
============================================================================
pdfcer has three render instruments and none of them measures colour accuracy:

  * `tools/render-parity` diffs whole pages against pdfium and reduces to
    `frac_over_T` — a fraction of pixels over a threshold. It cannot separate
    "the colour is 20 counts off everywhere" from "a glyph moved half a
    pixel", and its own `benign_structure.py` exists because that conflation
    already produced a wrong verdict once.
  * `tools/suite-check.py` reads the print-conformance suite's authored trap
    marks. Its criterion is the ABSENCE of a cross, which is a conformance
    question. A patch can pass it while every colour on it is wrong: two of
    them demonstrably do (`PCS3_132`, `PCS3_133`).
  * The `cmyk_intent.rs` tests pin specific conversions against values chosen
    when they were written. They are regression pins, not a comparison with
    anything outside pdfcer.

So the project has been carrying colour claims — most consequentially, that
`CmykIntent::Calibrated` shows *"what Acrobat shows, which is the point"* —
with no instrument that could confirm or refute them over more than one patch
at a time. This is that instrument.

WHAT IT MEASURES
================
For each patch with a reference render:

  1. Render the patch in pdfcer at the reference's **width**, so the two
     rasters are the same size to within the reference's own rounding.
  2. In the REFERENCE, find every FLAT REGION: a connected component of
     pixels sharing one exact 8-bit colour, of at least `--min-area` pixels.
  3. ERODE that region (`--erode`, default 3 px). This is the load-bearing
     step: the two rasters are NOT pixel-aligned — the reference's height and
     pdfcer's differ by a row or two — so an un-eroded region would sample
     pdfcer's antialiased boundary and report edge noise as colour error. That
     mistake has a name in this project's notes: a region picked without
     regard for its edges reported antialiasing as colour error AND hid two
     real defects, in one table.
  4. Read pdfcer's **median** colour over the eroded region, mapped by
     NORMALISED coordinates so a scale mismatch of a few tenths of a percent
     cannot shift the sample off the region.
  5. Report both colours and the per-channel signed delta.

Median rather than mean, deliberately: a mean over a region that has caught a
few boundary pixels drifts toward them, and the drift is invisible in the
output. A median over a genuinely flat interior IS the flat colour, and a
median that disagrees with the region's own mode says the interior was not
flat — which is reported (`flat=` on the row) rather than silently averaged.

WHAT A ROW DOES AND DOES NOT MEAN
=================================
A row says: *at this place, the reference painted this colour and pdfcer
painted that one.* It does **not** say which is right. The reference engine is
the project's parity target, not an oracle — where the standard leaves the
conversion device-dependent (§8.6.4.4 says exactly that for `DeviceCMYK`),
there is no correct answer to appeal to and a difference is a difference, not
an error.

★ It also does not say WHY. A row showing pdfcer 20 counts cool on a grey is
consistent with a conversion-table difference, with a different rendering
intent, and with the two engines disagreeing about which colour space the
content is in. Separating those needs `--probe-ink`, which reports the ink
BEFORE the conversion.

THE NEUTRALITY REPORT
=====================
`--neutrals` restricts the summary to regions the reference painted
ACHROMATIC (max channel spread <= `--neutral-tol`, default 1) and reports the
spread pdfcer put on the same region. This is the specific claim under test:
a conversion that is faithful on greys leaves a neutral neutral.

An achromatic reference region is the right population because it is
*decidable from the reference alone* and needs no assumption about what the
content stream said. A population selected by what pdfcer did would be
selected by the thing being measured.

USAGE
=====
    set PDFCER_SUITE_DIR=...        the patch PDFs
    set PDFCER_SUITE_REFS=...       the reference engine's renders, `<stem>.png`

    python tools/flat-color-parity.py [--neutrals] [--min-area 2000]
                                      [--erode 3] [--only PCS3_230] [--tsv out.tsv]

Out-of-tree tooling, exactly like `tools/render-parity`: never shipped, never
in `cargo test`, never in the GUI-core `cargo tree` invariant. Requires
`pillow`, `numpy` and `opencv-python`; uses the shipped `pdfcer
render-page`. The corpus lives OUTSIDE the repository (`docs/LEGAL.md` §5) and
this script writes nothing into it.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

import cv2
import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent


def cli_path() -> str:
    """The release binary, or a message saying how to get one.

    Release rather than debug for the same reason every other harness here
    uses it: a 51-patch sweep at reference resolution is minutes against
    hours, and the two produce identical pixels.
    """
    for cand in ("pdfcer.exe", "pdfcer"):
        p = ROOT / "target" / "release" / cand
        if p.exists():
            return str(p)
    sys.exit("build the release CLI first: cargo build --release -p pdfcer-cli")


def manifest() -> dict[str, str]:
    """`id -> filename`, read from `pdfcer-manifest.txt` beside the corpus.

    The corpus's file names are as much the licensed suite's material as its
    artwork is, so they are not written down in this repository. An absent
    manifest is a skip with a message, never a failure — a fresh clone has no
    corpus and must not fail here.
    """
    root = os.environ.get("PDFCER_SUITE_DIR")
    if not root:
        return {}
    path = Path(root) / "pdfcer-manifest.txt"
    if not path.exists():
        return {}
    out: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        k, v = line.split("=", 1)
        out[k.strip()] = v.strip()
    return out


def media_box(pdf: Path) -> tuple[float, float]:
    """The first `/MediaBox`'s width and height in points.

    A regex over the raw bytes rather than a parse: this tool needs one
    number to pick a render scale, the patches all carry a literal
    uncompressed `/MediaBox` on the page object, and depending on a parser
    here would mean this script could fail on a file `pdfcer` renders
    perfectly well.
    """
    b = pdf.read_bytes()
    m = re.search(rb"/MediaBox\s*\[\s*([-\d.]+)\s+([-\d.]+)\s+([-\d.]+)\s+([-\d.]+)", b)
    if not m:
        return (612.0, 792.0)
    llx, lly, urx, ury = (float(m.group(i)) for i in range(1, 5))
    return (abs(urx - llx), abs(ury - lly))


def render(pdf: Path, out: Path, scale: float) -> bool:
    """Rasterise one page with the shipped CLI. Returns success."""
    r = subprocess.run(
        [cli_path(), "render-page", "--scale", f"{scale:.6f}", "-o", str(out), str(pdf)],
        capture_output=True,
        text=True,
        check=False,
    )
    return r.returncode == 0 and out.exists()


def flat_regions(ref: np.ndarray, min_area: int, erode: int):
    """Yield `(colour, eroded_mask, area)` for each flat region of `ref`.

    Segmenting on EXACT 8-bit values rather than on a quantisation is the same
    choice `tools/suite-check.py` makes and for the same reason: two flat
    fills a few counts apart are different fills, and any bucketing wide
    enough to absorb antialiasing is also wide enough to merge them.

    The erosion is applied per connected component rather than to the whole
    colour's mask, so two touching regions of the same colour do not erode
    each other's shared boundary away.
    """
    flat = ref.reshape(-1, 3)
    colours, counts = np.unique(flat, axis=0, return_counts=True)
    kernel = np.ones((3, 3), np.uint8)
    for colour, count in zip(colours, counts):
        if count < min_area:
            continue
        mask = np.all(ref == colour, axis=2).astype(np.uint8)
        n, labels, stats, _ = cv2.connectedComponentsWithStats(mask, 8)
        for i in range(1, n):
            if stats[i, cv2.CC_STAT_AREA] < min_area:
                continue
            comp = (labels == i).astype(np.uint8)
            if erode > 0:
                comp = cv2.erode(comp, kernel, iterations=erode)
            area = int(comp.sum())
            if area < max(16, min_area // 20):
                # Eroded away: a long thin region, e.g. a rule or a stroke.
                # Reported as skipped rather than measured, because a sliver's
                # interior is all boundary and its colour is not its fill's.
                continue
            yield tuple(int(c) for c in colour), comp, area


def sample(pdfcer: np.ndarray, mask: np.ndarray) -> tuple[tuple[int, int, int], float]:
    """pdfcer's median colour over `mask`, mapped by NORMALISED coordinates.

    Returns the median and the fraction of sampled pixels that equal it — the
    `flat=` column. A fraction near 1.0 means pdfcer painted a flat region
    where the reference did; a low one means the sample straddled something,
    and the row's delta should not be read as a fill's colour.
    """
    mh, mw = mask.shape
    ph, pw = pdfcer.shape[:2]
    ys, xs = np.nonzero(mask)
    # Normalised mapping rather than a crop-to-common-extent: the two rasters
    # differ by a row or two out of several hundred, and cropping would put a
    # systematic offset on every region near the bottom of the page while
    # leaving the top exact -- a bias that varies with position is worse than
    # one that does not, because it looks like a real spatial pattern.
    py = np.clip((ys.astype(np.float64) * ph / mh).astype(int), 0, ph - 1)
    px = np.clip((xs.astype(np.float64) * pw / mw).astype(int), 0, pw - 1)
    vals = pdfcer[py, px]
    med = tuple(int(v) for v in np.median(vals, axis=0))
    same = float(np.mean(np.all(vals == np.array(med), axis=1)))
    return med, same


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--min-area", type=int, default=2000, help="minimum flat-region area, px")
    ap.add_argument("--erode", type=int, default=3, help="erosion iterations before sampling")
    ap.add_argument("--neutrals", action="store_true", help="summarise achromatic regions only")
    ap.add_argument("--neutral-tol", type=int, default=1, help="max channel spread for 'achromatic'")
    ap.add_argument("--only", action="append", default=[], help="restrict to these patch ids")
    ap.add_argument("--tsv", type=Path, help="write every row here")
    ap.add_argument("--json", type=Path, help="write the summary here")
    args = ap.parse_args()

    ids = manifest()
    refs = os.environ.get("PDFCER_SUITE_REFS")
    corpus = os.environ.get("PDFCER_SUITE_DIR")
    if not ids or not refs or not corpus:
        sys.exit(
            "SKIP: set PDFCER_SUITE_DIR (patches + pdfcer-manifest.txt) and "
            "PDFCER_SUITE_REFS (reference renders). The corpus is not "
            "redistributable and a fresh clone will not have it."
        )
    refdir, cdir = Path(refs), Path(corpus)
    tmp = Path(os.environ.get("TEMP", "/tmp")) / "flat-color-parity"
    tmp.mkdir(parents=True, exist_ok=True)

    rows = []
    for pid in sorted(ids):
        if args.only and pid not in args.only:
            continue
        pdf = cdir / ids[pid]
        ref_png = refdir / (Path(ids[pid]).stem + ".png")
        if not pdf.exists() or not ref_png.exists():
            print(f"SKIP {pid}: no patch or no reference render", file=sys.stderr)
            continue
        ref = np.array(Image.open(ref_png).convert("RGB"))
        pw_pt, _ = media_box(pdf)
        scale = ref.shape[1] / pw_pt
        out = tmp / f"{pid}.png"
        if not render(pdf, out, scale):
            print(f"SKIP {pid}: pdfcer could not render it", file=sys.stderr)
            continue
        got = np.array(Image.open(out).convert("RGB"))
        for colour, mask, area in flat_regions(ref, args.min_area, args.erode):
            med, flatness = sample(got, mask)
            spread_ref = max(colour) - min(colour)
            spread_got = max(med) - min(med)
            rows.append(
                {
                    "patch": pid,
                    "area": area,
                    "ref": colour,
                    "pdfcer": med,
                    "delta": [m - c for m, c in zip(med, colour)],
                    "ref_spread": spread_ref,
                    "pdfcer_spread": spread_got,
                    "flat": round(flatness, 3),
                }
            )
        print(f"{pid}: {sum(1 for r in rows if r['patch'] == pid)} flat region(s)", file=sys.stderr)

    if args.neutrals:
        rows = [r for r in rows if r["ref_spread"] <= args.neutral_tol]

    header = "patch\tarea\tref\tpdfcer\tdelta\tref_spread\tpdfcer_spread\tflat"
    lines = [header]
    for r in sorted(rows, key=lambda r: (-max(abs(d) for d in r["delta"]), r["patch"])):
        lines.append(
            "\t".join(
                [
                    r["patch"],
                    str(r["area"]),
                    ",".join(map(str, r["ref"])),
                    ",".join(map(str, r["pdfcer"])),
                    ",".join(f"{d:+d}" for d in r["delta"]),
                    str(r["ref_spread"]),
                    str(r["pdfcer_spread"]),
                    f"{r['flat']:.3f}",
                ]
            )
        )
    text = "\n".join(lines)
    if args.tsv:
        args.tsv.write_text(text + "\n", encoding="utf-8")
    print(text)

    if rows:
        worst = [max(abs(d) for d in r["delta"]) for r in rows]
        summary = {
            "regions": len(rows),
            "max_channel_delta": max(worst),
            "median_channel_delta": int(np.median(worst)),
            "regions_exact": sum(1 for w in worst if w == 0),
            "neutrals_only": bool(args.neutrals),
        }
        if args.neutrals:
            summary["pdfcer_spread_max"] = max(r["pdfcer_spread"] for r in rows)
            summary["pdfcer_spread_median"] = int(np.median([r["pdfcer_spread"] for r in rows]))
            summary["neutral_in_neutral_out"] = sum(
                1 for r in rows if r["pdfcer_spread"] <= args.neutral_tol
            )
        print("\n" + json.dumps(summary, indent=2), file=sys.stderr)
        if args.json:
            args.json.write_text(json.dumps(summary, indent=2), encoding="utf-8")


if __name__ == "__main__":
    main()
