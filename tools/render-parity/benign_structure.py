#!/usr/bin/env python3
"""benign_structure — audit the `benign-renderer-noise` bucket's NAME.

WHY THIS EXISTS
===============
`render_parity.py` classifies every (file, page) into three buckets. The
first, `benign-renderer-noise`, is assigned by exactly one test:

    frac_over_32 <= band

and nothing else. `frac_over_32` is the FRACTION of pixels whose max
per-channel delta exceeds 32/255. The bucket's NAME, however, asserts a
CAUSE — anti-aliasing, font hinting, sub-pixel glyph positioning, image
interpolation. A measurement below a threshold is not evidence of a cause.

The `render_parity.py` module docstring states the reasoning that is supposed
to bridge measurement and cause (step 1 of "HOW THE TOLERANCE BAND IS
DERIVED"):

    benign AA/hinting noise is confined to a THIN sub-pixel band around
    edges, so it touches a SMALL fraction of the page even where individual
    edge pixels swing the full 0..255; a real divergence -- a missing shading
    fill, a wrong DeviceCMYK colour, a shifted glyph run -- touches a LARGE
    contiguous AREA, i.e. a large fraction.

That is a claim about SPATIAL STRUCTURE ("thin band around edges",
"contiguous area"). `frac_over_32` measures no structure whatsoever: it is a
pixel count divided by a pixel count. A page can have a small total fraction
that is nonetheless one solid contiguous blob — a missing 40x40pt logo on an
A4 page is ~0.7% of the page area and lands two orders of magnitude inside
the band. Such a page is called benign by construction and no one ever looks
at it.

Worse, the failure compounds. The band is the p99.9 of `frac_over_32` over
"clean-by-construction" pages — pages for which pdfcer disclosed ZERO gaps.
A bug pdfcer does not KNOW about emits no diagnostic, so its page is counted
clean-by-construction, and its divergence is folded into the population that
DEFINES the band. A silent bug does not merely escape detection; it raises
the threshold that hides other bugs.

This tool tests the docstring's own discriminator, on the docstring's own
terms, by computing the structure `frac_over_32` throws away.

THE DISCRIMINATOR: SHARED-EDGE EXPLANATION
==========================================
Every mechanism the bucket name asserts (AA, hinting, sub-pixel positioning,
image interpolation) has one property in common and it is checkable: the
divergence lies ON A FEATURE BOUNDARY THAT BOTH ENGINES DREW. Two renderers
disagreeing about how to cover a pixel that a glyph stem's edge passes
through can only disagree where that edge exists -- in BOTH rasters.

So for each page:

  1. `over`   = pixels with max-channel delta > 32 (exactly the population
                `frac_over_32` counts, so this audits the metric's own
                pixels, not a different set).
  2. `edge_a` = per-channel morphological gradient (3x3 max - 3x3 min) of the
                PDFCER raster exceeds 32; `edge_b` = the same for the PDFIUM
                raster. Per-channel and then max-across-channels, because an
                isoluminant colour boundary (red|green) has a real edge and
                no luminance gradient.
  3. `shared` = dilate(edge_a, r) AND dilate(edge_b, r), r = 2 px. A pixel is
                "shared-edge-explained" if BOTH engines drew a feature
                boundary within 2 px of it.
  4. `off`    = over AND NOT shared -- over-threshold pixels that no shared
                boundary can account for.

Why the AND of both engines, rather than the union: it is what separates
"the two engines shaded a mutually-drawn edge differently" (benign) from
"one engine drew something the other did not" (not benign). It catches four
distinct real-divergence shapes that a union test or a pure size test misses:

  * MISSING OBJECT   -- pdfium draws a filled shape, pdfcer leaves the area
                        blank. The shape's INTERIOR is uniform in both
                        rasters, so it is edge-free in both; every interior
                        pixel lands in `off`.
  * WRONG COLOUR     -- the object is present in both and its boundary IS a
                        shared edge, but its interior is flat in both, so
                        interior over-pixels are far from any edge -> `off`.
                        (This is deliberately NOT excused: a wrong colour is
                        a real divergence, e.g. the DeviceCMYK colorimetry
                        gap.)
  * SHIFTED CONTENT  -- a text block displaced by more than a couple of px
                        puts glyph ink where the other engine has paper. At
                        the vacated position pdfcer has an edge and pdfium
                        does not; at the new position the reverse. Neither
                        position is a SHARED edge, so both land in `off`.
                        A union-of-edges test would excuse this entirely;
                        the AND does not.
  * SPURIOUS OBJECT  -- symmetric to missing.

whereas the mechanisms the name asserts score high on `shared`:

  * AA / hinting / sub-pixel positioning -- both engines drew the same glyph
    at the same place and disagree only about coverage of the boundary
    pixels; those pixels are within 2 px of an edge in both rasters.
  * image interpolation -- a photographic region has high local gradient
    nearly everywhere in both rasters, so its resampling noise is
    edge-explained. Correct: the docstring names interpolation as benign.

5. CONTIGUITY. `off` alone is not enough, because a fine dither, a hairline
   hatch, or JPEG ringing can leave a scatter of isolated off-edge pixels
   that is genuinely noise. So the reported statistic is the LARGEST
   CONNECTED COMPONENT (8-connectivity) of `off`, in pixels -- `off_lcc_px`.
   Speckle has a tiny LCC by construction; a missing logo has an LCC the
   size of the logo. This is the "contiguous AREA" half of the docstring's
   claim, measured.

WHAT THIS TOOL DOES NOT DECIDE
==============================
`off_lcc_px` is EVIDENCE, not a verdict. The tool ranks pages by it and
emits a 4-panel image for the top of the ranking and for a random control
sample; a human (or a vision model) then looks. No page is called a bug by a
threshold alone -- that would repeat the exact error being audited.

Known limits, stated rather than papered over:

  * Sub-2px displacement of text is shared-edge-explained by design (r = 2).
    That is the intended reading of "sub-pixel positioning is benign", but it
    means this tool cannot distinguish a 1px systematic offset from AA.
  * A divergence confined ENTIRELY to a genuinely edge-dense region (dense
    small text, a photographic image) is edge-explained and will not be
    flagged. This tool bounds the FALSE-BENIGN rate for area-shaped
    divergence; it does not bound it for texture-shaped divergence.
  * `edge_thr = 32` and `r = 2` are declared before the run and never
    re-tuned. `--edge-thr` / `--edge-radius` exist so the SENSITIVITY of the
    finding can be reported, not so the finding can be improved.

OUTPUTS
=======
  <outdir>/structure.tsv          one row per analysed page, sorted by
                                  off_lcc_px descending. Every statistic,
                                  never just the flag.
  <outdir>/structure-summary.txt  distributions, the flag counts at several
                                  thresholds (sensitivity, not one number),
                                  and the ranked head of the table.
  <outdir>/structure-summary.json machine-readable twin.
  <outdir>/panels/*.png           4-panel triage images:
                                  [pdfcer | pdfium | 8x delta | classification]
                                  where the classification panel paints
                                  shared-edge-explained over-pixels GREEN and
                                  unexplained ones RED over a dimmed pdfium
                                  raster, so "where is the divergence and is
                                  it edge-shaped" is answerable at a glance.

METRIC-DRIFT GUARD
==================
The recorded run pinned its own copy of the CLI (`pdfcer-pinned.exe`),
which no longer exists. This tool therefore re-renders with the CURRENT
`target/release/pdfcer` and records BOTH the recomputed `frac32` and the
`frac32` the baseline recorded, plus their delta. If the binary has moved,
the report says so instead of silently comparing two different programs.

USAGE
=====
    python benign_structure.py --tsv out-corpus-4023/per-page.tsv \
        --outdir out-benign-audit [--all | --stratified N] [--bucket benign]

Requires: numpy, scipy, Pillow, a built `pdfcer` release binary, and
`pdfium_worker.py` beside this file (PDFium contact stays in a child process
for exactly the reason `render_parity.py` documents -- it aborts the host on
at least one corpus file).
"""

from __future__ import annotations

import argparse
import csv
import json
import random
import subprocess
import sys
import tempfile
import time
from collections import Counter
from dataclasses import asdict, dataclass
from pathlib import Path

import numpy as np
from PIL import Image
from scipy import ndimage

# Reuse the harness's rendering/compare primitives verbatim so the pixels this
# tool measures are the SAME pixels render_parity measured. Importing rather
# than re-implementing is deliberate: a re-implementation that drifted (a
# different alpha compositing, a different scale rounding) would audit a
# different raster than the one the baseline bucketed.
import render_parity as rp

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent

# --- declared-in-advance analysis constants --------------------------------
# These are fixed BEFORE any page is looked at. The CLI flags that override
# them exist to report sensitivity of the conclusion, never to improve it.
PIXEL_DELTA_T = 32   # identical to render_parity.PIXEL_DELTA_T, by import below
EDGE_THR = 32        # morphological-gradient threshold for "a feature boundary"
EDGE_RADIUS = 2      # px; how far AA/hinting noise may sit from that boundary

# 8-connectivity: a divergence region is contiguous if it touches diagonally.
# 4-connectivity would split a 1px-wide diagonal streak into a dust of
# components and understate contiguity.
CONN8 = np.ones((3, 3), dtype=bool)


@dataclass
class StructResult:
    """Every structural statistic for one analysed page.

    Deliberately wide: the report must be able to show the DISTRIBUTION of
    each statistic, not just the pages that tripped a flag. A tool that only
    records its own flag cannot be checked for having the flag wrong.
    """

    rel: str
    page: int
    bucket: str          # bucket the baseline assigned
    clean: int           # 1 => the page helped DEFINE the band
    status: str = "ok"   # "ok" | "error"
    note: str = ""

    w: int = 0
    h: int = 0
    page_px: int = 0

    frac32_recorded: float = 0.0   # from the baseline TSV
    frac32_now: float = 0.0        # recomputed with the current binary
    frac32_drift: float = 0.0

    n_over: int = 0                # pixels with delta > 32 (what frac32 counts)
    n_off: int = 0                 # of those, not shared-edge-explained
    shared_edge_frac: float = 0.0  # 1 - n_off/n_over

    over_lcc_px: int = 0           # largest contiguous over-threshold region
    over_lcc_frac_of_over: float = 0.0
    n_over_components: int = 0

    off_lcc_px: int = 0            # THE headline statistic
    off_lcc_frac_of_page: float = 0.0
    off_lcc_bbox: str = ""         # "x0,y0,x1,y1"
    off_lcc_solidity: float = 0.0  # lcc_px / bbox area; 1.0 = a solid block
    off_lcc_maxthick: float = 0.0  # max distance-to-boundary inside the blob
    n_off_components: int = 0

    over_maxthick: float = 0.0     # max EDT over the whole over-mask
    over_thick2_frac: float = 0.0  # share of over pixels >=2px deep (>=5px wide)

    # What the two engines actually painted inside the largest unexplained
    # blob. This is what turns "there is a blob" into a diagnosis: pdfcer
    # (255,255,255) vs pdfium (30,60,180) reads "pdfcer drew nothing".
    off_lcc_pdfcer_rgb: str = ""
    off_lcc_pdfium_rgb: str = ""
    off_lcc_mean_delta: float = 0.0


def morph_edges(img: np.ndarray, thr: int) -> np.ndarray:
    """Boolean map of "a feature boundary passes through this pixel".

    Per-channel 3x3 morphological gradient (local max - local min), thresholded
    at `thr`, OR-ed across channels.

    WHY per-channel and not on luminance: an isoluminant colour boundary --
    a red field abutting a green field of equal luminance -- is a real feature
    boundary with essentially zero luminance gradient. A luminance-only edge
    map would call the AA fringe along such a boundary "off-edge" and produce
    a false bug candidate. Doing it per-channel costs 3x the filter work and
    removes the failure mode.

    WHY a morphological gradient and not Sobel: max-min is exactly "the pixel
    values within this 3x3 neighbourhood span more than `thr`", which is the
    property that matters here (two engines can disagree by up to that span
    when they distribute coverage differently). Sobel's directional weighting
    would attenuate single-pixel hairlines, which are precisely where AA noise
    lives.
    """
    out = np.zeros(img.shape[:2], dtype=bool)
    for c in range(img.shape[2]):
        ch = img[:, :, c]
        hi = ndimage.maximum_filter(ch, size=3, mode="nearest")
        lo = ndimage.minimum_filter(ch, size=3, mode="nearest")
        out |= (hi.astype(np.int16) - lo.astype(np.int16)) > thr
    return out


def dilate(mask: np.ndarray, radius: int) -> np.ndarray:
    """Boolean dilation by a square structuring element of the given radius.

    `maximum_filter` on a boolean array IS binary dilation, and is far faster
    than `binary_dilation` with an explicit structure for square elements.
    A square (Chebyshev) neighbourhood rather than a disc is intentional: it
    is the more GENEROUS of the two, so it excuses more pixels as edge noise.
    Every choice in this tool that could go either way is made in the
    direction that makes a bug HARDER to claim.
    """
    if radius <= 0:
        return mask
    # uint8 rather than bool: scipy's `maximum_filter` rejects a boolean
    # `cval`, and a boolean input array is not portable across scipy versions
    # for this filter. 0/255 keeps it exact and cheap.
    m = mask.astype(np.uint8)
    return ndimage.maximum_filter(m, size=2 * radius + 1, mode="constant", cval=0) > 0


def largest_component(mask: np.ndarray) -> tuple[int, int, tuple[int, int, int, int] | None, np.ndarray | None]:
    """(n_components, largest_size_px, bbox, largest_component_mask).

    bbox is (x0, y0, x1, y1) inclusive. Returns (0, 0, None, None) for an
    empty mask. 8-connectivity per CONN8 -- see its comment.
    """
    if not mask.any():
        return 0, 0, None, None
    lab, n = ndimage.label(mask, structure=CONN8)
    if n == 0:
        return 0, 0, None, None
    # np.bincount over labels; index 0 is background.
    counts = np.bincount(lab.ravel())
    counts[0] = 0
    idx = int(counts.argmax())
    size = int(counts[idx])
    # `find_objects` wants the INTEGER label array (a boolean mask raises
    # "'numpy.bool' object cannot be interpreted as an integer"); slice
    # `idx-1` is the bounding box of label `idx`.
    sl = ndimage.find_objects(lab)[idx - 1]
    y0, y1 = sl[0].start, sl[0].stop - 1
    x0, x1 = sl[1].start, sl[1].stop - 1
    return int(n), size, (x0, y0, x1, y1), (lab == idx)


def analyse(
    pdfcer: np.ndarray, pdfium: np.ndarray, delta: np.ndarray,
    pixel_t: int, edge_thr: int, edge_radius: int,
) -> dict:
    """Compute the full structural statistic set for one page pair.

    Order of operations matters for cost: the over-mask is computed first and
    the whole analysis short-circuits when it is empty, because roughly an
    eighth of the benign population has literally zero over-threshold pixels
    and building four edge maps for them is pure waste.
    """
    h = min(pdfcer.shape[0], pdfium.shape[0], delta.shape[0])
    w = min(pdfcer.shape[1], pdfium.shape[1], delta.shape[1])
    a = pdfcer[:h, :w, :]
    b = pdfium[:h, :w, :]
    d = delta[:h, :w]

    over = d > pixel_t
    n_over = int(over.sum())
    res: dict = {
        "w": w, "h": h, "page_px": h * w,
        "n_over": n_over, "n_off": 0, "shared_edge_frac": 1.0,
        "over_lcc_px": 0, "over_lcc_frac_of_over": 0.0, "n_over_components": 0,
        "off_lcc_px": 0, "off_lcc_frac_of_page": 0.0, "off_lcc_bbox": "",
        "off_lcc_solidity": 0.0, "off_lcc_maxthick": 0.0, "n_off_components": 0,
        "over_maxthick": 0.0, "over_thick2_frac": 0.0,
        "off_lcc_pdfcer_rgb": "", "off_lcc_pdfium_rgb": "", "off_lcc_mean_delta": 0.0,
        "_masks": None,
    }
    if n_over == 0:
        return res

    shared = dilate(morph_edges(a, edge_thr), edge_radius) & \
        dilate(morph_edges(b, edge_thr), edge_radius)
    off = over & ~shared
    n_off = int(off.sum())
    res["n_off"] = n_off
    res["shared_edge_frac"] = 1.0 - n_off / n_over

    # Thickness of the whole over-mask. The docstring's "THIN sub-pixel band"
    # is a claim about this: a 1-3px halo has an EDT max of ~1-1.5, a solid
    # region has an EDT max of half its narrowest width.
    edt_over = ndimage.distance_transform_edt(over)
    res["over_maxthick"] = float(edt_over.max())
    res["over_thick2_frac"] = float((edt_over[over] >= 2.0).mean())

    n_c, sz, bbox, _ = largest_component(over)
    res["n_over_components"] = n_c
    res["over_lcc_px"] = sz
    res["over_lcc_frac_of_over"] = sz / n_over if n_over else 0.0

    if n_off:
        n_c, sz, bbox, lccmask = largest_component(off)
        res["n_off_components"] = n_c
        res["off_lcc_px"] = sz
        res["off_lcc_frac_of_page"] = sz / (h * w)
        if bbox is not None:
            x0, y0, x1, y1 = bbox
            res["off_lcc_bbox"] = f"{x0},{y0},{x1},{y1}"
            area = (x1 - x0 + 1) * (y1 - y0 + 1)
            res["off_lcc_solidity"] = sz / area if area else 0.0
        if lccmask is not None:
            edt = ndimage.distance_transform_edt(lccmask)
            res["off_lcc_maxthick"] = float(edt.max())
            res["off_lcc_pdfcer_rgb"] = ",".join(
                str(int(round(v))) for v in a[lccmask].mean(axis=0))
            res["off_lcc_pdfium_rgb"] = ",".join(
                str(int(round(v))) for v in b[lccmask].mean(axis=0))
            res["off_lcc_mean_delta"] = float(d[lccmask].mean())
        res["_masks"] = (over, shared)
    else:
        res["_masks"] = (over, shared)
    return res


def classification_panel(
    pdfcer: np.ndarray, pdfium: np.ndarray, delta: np.ndarray,
    over: np.ndarray, shared: np.ndarray,
) -> Image.Image:
    """[pdfcer | pdfium | 8x delta | classification] as one RGB image.

    The fourth panel is the point of the whole tool: it dims the pdfium raster
    to 25% so the page is still legible as context, then paints every
    over-threshold pixel GREEN if a shared edge explains it and RED if nothing
    does. A page whose red pixels form a recognisable SHAPE is a bug
    candidate; a page whose red is a dust of single pixels is not. That
    judgement is made by looking, which is what the audited bucket never had.
    """
    h = min(pdfcer.shape[0], pdfium.shape[0], delta.shape[0], over.shape[0])
    w = min(pdfcer.shape[1], pdfium.shape[1], delta.shape[1], over.shape[1])
    a, b = pdfcer[:h, :w, :], pdfium[:h, :w, :]
    d = np.clip(delta[:h, :w].astype(np.int32) * 8, 0, 255).astype(np.uint8)
    dmap = np.stack([d, d, d], axis=2)

    cls = (b.astype(np.float32) * 0.25 + 191).clip(0, 255).astype(np.uint8)
    ov, sh = over[:h, :w], shared[:h, :w]
    on = ov & sh
    off = ov & ~sh
    cls[on] = (0, 170, 0)
    cls[off] = (255, 0, 0)

    gap = np.full((h, 8, 3), 120, dtype=np.uint8)
    return Image.fromarray(
        np.concatenate([a, gap, b, gap, dmap, gap, cls], axis=1), "RGB")


def safe_name(rel: str, page: int, prefix: str) -> str:
    s = rel.replace("/", "_").replace("\\", "_")
    for ch in '<>:"|?*':
        s = s.replace(ch, "_")
    return f"{prefix}_{s}_p{page}.png"[:200]


def load_rows(tsv: Path, bucket: str | None) -> list[dict]:
    with tsv.open(encoding="utf-8", newline="") as fh:
        rows = list(csv.DictReader(fh, delimiter="\t"))
    if bucket:
        rows = [r for r in rows if r.get("bucket") == bucket]
    return [r for r in rows if r.get("status") == "ok"]


def stratified_sample(rows: list[dict], n: int, seed: int) -> list[dict]:
    """Sample `n` rows stratified by frac32 decile AND by source corpus.

    WHY stratified and not "the first N" or a flat random draw: the population
    is dominated by near-zero pages (13% are exactly zero, the median is
    0.00054) and by one corpus (veraPDF is 78% of it). A flat draw would put
    almost the entire sample in the boring middle of the biggest corpus, and
    the interesting region -- the top decile, just under the band, where a
    real divergence would hide -- would get a handful of pages by luck. So:
    ten equal-count frac32 strata, and within each stratum a round-robin over
    corpora before any corpus repeats. Reported exactly this way; a sample is
    never presented as a census.
    """
    rng = random.Random(seed)
    ordered = sorted(rows, key=lambda r: float(r["frac32"]))
    k = 10
    per = max(1, n // k)
    picked: list[dict] = []
    for i in range(k):
        lo = i * len(ordered) // k
        hi = (i + 1) * len(ordered) // k
        stratum = ordered[lo:hi]
        by_corpus: dict[str, list[dict]] = {}
        for r in stratum:
            parts = r["file"].split("/")
            corpus = parts[1] if len(parts) > 1 else parts[0]
            by_corpus.setdefault(corpus, []).append(r)
        for v in by_corpus.values():
            rng.shuffle(v)
        keys = sorted(by_corpus)
        take: list[dict] = []
        idx = 0
        while len(take) < min(per, len(stratum)):
            kk = keys[idx % len(keys)]
            if by_corpus[kk]:
                take.append(by_corpus[kk].pop())
            elif all(not by_corpus[x] for x in keys):
                break
            idx += 1
        picked.extend(take)
    return picked


def main(argv: list[str] | None = None) -> int:
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="replace")

    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--tsv", default="out-corpus-4023/per-page.tsv")
    ap.add_argument("--outdir", default="out-benign-audit")
    ap.add_argument("--corpus-root", default=str(ROOT / "fixtures"))
    ap.add_argument("--bucket", default="benign",
                    help="baseline bucket to audit ('' = all ok rows)")
    ap.add_argument("--all", action="store_true", help="census of the bucket")
    ap.add_argument("--stratified", type=int, default=0,
                    help="stratified sample size (frac32 decile x corpus)")
    ap.add_argument("--seed", type=int, default=20260808)
    ap.add_argument("--dpi", type=float, default=125.0)
    ap.add_argument("--annots", action="store_true", default=False)
    ap.add_argument("--pixel-t", type=int, default=PIXEL_DELTA_T)
    ap.add_argument("--edge-thr", type=int, default=EDGE_THR)
    ap.add_argument("--edge-radius", type=int, default=EDGE_RADIUS)
    ap.add_argument("--panels-top", type=int, default=40,
                    help="emit panels for the N pages with the largest off_lcc_px")
    ap.add_argument("--panels-control", type=int, default=12,
                    help="emit panels for N randomly chosen pages regardless of rank")
    ap.add_argument("--panel-min-off-lcc", type=int, default=1,
                    help="do not emit a top-panel for a page below this off_lcc_px")
    ap.add_argument("--timeout", type=float, default=60.0)
    ap.add_argument("--limit", type=int, default=0, help="debug: stop after N pages")
    args = ap.parse_args(argv)

    tsv = Path(args.tsv)
    if not tsv.is_absolute():
        tsv = HERE / tsv
    outdir = Path(args.outdir)
    if not outdir.is_absolute():
        outdir = HERE / outdir
    outdir.mkdir(parents=True, exist_ok=True)
    (outdir / "panels").mkdir(exist_ok=True)
    corpus_root = Path(args.corpus_root)

    if not rp.CLI.exists():
        print(f"ERROR: pdfcer not found at {rp.CLI}", file=sys.stderr)
        return 2

    rows = load_rows(tsv, args.bucket or None)
    if args.stratified:
        sel = stratified_sample(rows, args.stratified, args.seed)
        sampling = (f"stratified: {len(sel)} of {len(rows)} rows, 10 equal-count "
                    f"frac32 strata x round-robin over corpora, seed={args.seed}")
    else:
        sel = list(rows)
        sampling = f"census: all {len(sel)} rows of bucket '{args.bucket}'"
    if args.limit:
        sel = sel[: args.limit]
        sampling += f" (debug --limit {args.limit})"

    scale = args.dpi / 72.0
    worker = rp.PdfiumWorker(timeout=args.timeout)
    results: list[StructResult] = []
    panels_wanted: dict[tuple[str, int], str] = {}
    rng = random.Random(args.seed ^ 0x5EED)
    control_keys = set()
    if args.panels_control and len(sel) > args.panels_control:
        for r in rng.sample(sel, args.panels_control):
            control_keys.add((r["file"], int(r["page"])))

    t0 = time.time()
    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td)
        for i, r in enumerate(sel):
            rel = r["file"]
            pg = int(r["page"])
            abs_ = corpus_root / rel
            sr = StructResult(rel=rel, page=pg, bucket=r.get("bucket", ""),
                              clean=int(r.get("clean", "0") or 0),
                              frac32_recorded=float(r["frac32"]))
            try:
                pdfcer_img, _diag = rp.render_pdfce(
                    abs_, pg, scale, args.annots, tmp, args.timeout)
                pdfium_img = worker.render(
                    abs_, pg - 1, scale, args.annots, tmp / "pdfium.raw")
                delta, stats, _dm = rp.compare(pdfcer_img, pdfium_img)
                st = analyse(pdfcer_img, pdfium_img, delta,
                             args.pixel_t, args.edge_thr, args.edge_radius)
                masks = st.pop("_masks")
                sr.frac32_now = stats["frac32"]
                sr.frac32_drift = sr.frac32_now - sr.frac32_recorded
                for k, v in st.items():
                    setattr(sr, k, v)
                key = (rel, pg)
                if key in control_keys and masks is not None:
                    p = outdir / "panels" / safe_name(rel, pg, f"ctl_{sr.off_lcc_px:06d}")
                    classification_panel(pdfcer_img, pdfium_img, delta, *masks).save(p)
                    panels_wanted[key] = p.name
            except rp.ReferenceAborted as exc:
                worker.kill()
                sr.status, sr.note = "error", f"pdfium-abort: {exc.cause}"
            except rp.ReferenceTimeout as exc:
                worker.kill()
                sr.status, sr.note = "error", f"pdfium-hang: {exc}"
            except subprocess.TimeoutExpired:
                sr.status, sr.note = "error", "pdfcer-timeout"
            except Exception as exc:  # noqa: BLE001
                sr.status, sr.note = "error", str(exc)[:120]
            results.append(sr)
            if (i + 1) % 100 == 0:
                el = time.time() - t0
                print(f"  [{i+1}/{len(sel)}] {el:.0f}s "
                      f"({el/(i+1):.2f}s/page)", flush=True)

        # Second pass: panels for the top-ranked pages (re-render; cheap
        # relative to keeping thousands of rasters alive in RAM).
        ok = [s for s in results if s.status == "ok"]
        top = sorted(ok, key=lambda s: -s.off_lcc_px)[: args.panels_top]
        for sr in top:
            if sr.off_lcc_px < args.panel_min_off_lcc:
                continue
            key = (sr.rel, sr.page)
            if key in panels_wanted:
                continue
            try:
                pdfcer_img, _ = rp.render_pdfce(
                    corpus_root / sr.rel, sr.page, scale, args.annots, tmp, args.timeout)
                pdfium_img = worker.render(
                    corpus_root / sr.rel, sr.page - 1, scale, args.annots,
                    tmp / "pdfium.raw")
                delta, _stats, _ = rp.compare(pdfcer_img, pdfium_img)
                st = analyse(pdfcer_img, pdfium_img, delta,
                             args.pixel_t, args.edge_thr, args.edge_radius)
                masks = st.pop("_masks")
                if masks is None:
                    continue
                p = outdir / "panels" / safe_name(sr.rel, sr.page, f"top_{sr.off_lcc_px:06d}")
                classification_panel(pdfcer_img, pdfium_img, delta, *masks).save(p)
                panels_wanted[key] = p.name
            except Exception as exc:  # noqa: BLE001
                print(f"  panel failed {sr.rel} p{sr.page}: {exc}", file=sys.stderr)
    worker.close()

    write_reports(results, outdir, args, sampling, time.time() - t0, panels_wanted)
    return 0


def _dist(vals: list[float]) -> dict:
    if not vals:
        return {"n": 0}
    a = np.asarray(vals, dtype=float)
    return {
        "n": int(a.size), "mean": float(a.mean()),
        "p50": float(np.percentile(a, 50)), "p90": float(np.percentile(a, 90)),
        "p99": float(np.percentile(a, 99)), "max": float(a.max()),
    }


def write_reports(results: list[StructResult], outdir: Path, args,
                  sampling: str, elapsed: float, panels: dict) -> None:
    """Emit structure.tsv + the txt/json summaries.

    The summary reports the flag count at SEVERAL off_lcc_px thresholds rather
    than one, deliberately. A single threshold would reproduce the failure
    being audited (a name asserted from a cutoff); a ladder lets the reader
    see whether the conclusion is robust or an artefact of where the line was
    drawn.
    """
    ok = [s for s in results if s.status == "ok"]
    err = [s for s in results if s.status != "ok"]
    ok.sort(key=lambda s: (-s.off_lcc_px, -s.n_off, s.rel))

    fields = list(asdict(ok[0]).keys()) if ok else list(asdict(
        StructResult("", 0, "", 0)).keys())
    with (outdir / "structure.tsv").open("w", encoding="utf-8", newline="\n") as fh:
        fh.write("\t".join(fields) + "\n")
        for s in ok + err:
            d = asdict(s)
            fh.write("\t".join(
                f"{d[k]:.6f}" if isinstance(d[k], float) else str(d[k])
                for k in fields) + "\n")

    ladder = [10, 25, 50, 100, 200, 400, 1000, 2500, 5000]
    counts = {t: sum(1 for s in ok if s.off_lcc_px >= t) for t in ladder}
    drift = [abs(s.frac32_drift) for s in ok]

    J = {
        "audit": "benign-renderer-noise bucket structural audit",
        "source_tsv": str(args.tsv),
        "bucket_audited": args.bucket,
        "sampling": sampling,
        "analysed_ok": len(ok),
        "errors": len(err),
        "elapsed_s": round(elapsed, 1),
        "constants": {
            "pixel_delta_threshold": args.pixel_t,
            "edge_threshold": args.edge_thr,
            "edge_radius_px": args.edge_radius,
            "connectivity": 8,
            "dpi": args.dpi,
        },
        "metric_drift_vs_recorded_frac32": _dist(drift),
        "distributions": {
            "frac32_now": _dist([s.frac32_now for s in ok]),
            "shared_edge_frac": _dist([s.shared_edge_frac for s in ok if s.n_over]),
            "n_off": _dist([float(s.n_off) for s in ok]),
            "off_lcc_px": _dist([float(s.off_lcc_px) for s in ok]),
            "off_lcc_frac_of_page": _dist([s.off_lcc_frac_of_page for s in ok]),
            "over_maxthick": _dist([s.over_maxthick for s in ok if s.n_over]),
            "over_thick2_frac": _dist([s.over_thick2_frac for s in ok if s.n_over]),
        },
        "off_lcc_threshold_ladder": counts,
        "top": [
            {
                "file": s.rel, "page": s.page,
                "frac32": round(s.frac32_now, 5),
                "off_lcc_px": s.off_lcc_px,
                "off_lcc_frac_of_page": round(s.off_lcc_frac_of_page, 6),
                "off_lcc_solidity": round(s.off_lcc_solidity, 3),
                "off_lcc_maxthick": round(s.off_lcc_maxthick, 2),
                "shared_edge_frac": round(s.shared_edge_frac, 4),
                "bbox": s.off_lcc_bbox,
                "pdfcer_rgb": s.off_lcc_pdfcer_rgb,
                "pdfium_rgb": s.off_lcc_pdfium_rgb,
                "clean_by_construction": s.clean,
                "panel": panels.get((s.rel, s.page), ""),
            }
            for s in ok[:60]
        ],
        "errors_detail": [{"file": s.rel, "page": s.page, "note": s.note} for s in err[:40]],
    }
    (outdir / "structure-summary.json").write_text(
        json.dumps(J, indent=2), encoding="utf-8")

    L: list[str] = []
    L.append("=== benign-bucket structural audit ===")
    L.append(f"source: {args.tsv}   bucket: {args.bucket!r}")
    L.append(f"sampling: {sampling}")
    L.append(f"analysed: {len(ok)} ok, {len(err)} errors, {elapsed:.0f}s")
    L.append(f"constants (declared before the run): pixel-delta>{args.pixel_t}, "
             f"edge-grad>{args.edge_thr}, edge-radius={args.edge_radius}px, 8-conn")
    L.append("")
    L.append("--- metric drift vs the recorded baseline (current CLI vs pinned) ---")
    d = J["metric_drift_vs_recorded_frac32"]
    if d.get("n"):
        L.append(f"  |frac32_now - frac32_recorded|: mean={d['mean']:.6f} "
                 f"p99={d['p99']:.6f} max={d['max']:.6f}")
    L.append("")
    L.append("--- distributions ---")
    for k, v in J["distributions"].items():
        if v.get("n"):
            L.append(f"  {k:22s} n={v['n']:5d} mean={v['mean']:.5f} p50={v['p50']:.5f} "
                     f"p90={v['p90']:.5f} p99={v['p99']:.5f} max={v['max']:.5f}")
    L.append("")
    L.append("--- pages whose over-threshold pixels are NOT shared-edge-explained ---")
    L.append("    (largest contiguous unexplained region, threshold ladder)")
    for t in ladder:
        pct = 100.0 * counts[t] / len(ok) if ok else 0.0
        L.append(f"  off_lcc_px >= {t:5d} : {counts[t]:5d} pages ({pct:.2f}%)")
    L.append("")
    L.append("--- ranked head (worst-first by off_lcc_px) ---")
    for e in J["top"][:40]:
        L.append(f"  off_lcc={e['off_lcc_px']:7d}px ({e['off_lcc_frac_of_page']*100:5.2f}% of page) "
                 f"sol={e['off_lcc_solidity']:.2f} thick={e['off_lcc_maxthick']:5.1f} "
                 f"sharedEdge={e['shared_edge_frac']:.3f} frac32={e['frac32']:.5f} "
                 f"clean={e['clean_by_construction']}")
        L.append(f"      {e['file']} p{e['page']}  bbox={e['bbox']} "
                 f"pdfcer={e['pdfcer_rgb']} pdfium={e['pdfium_rgb']}")
    if err:
        L.append("")
        L.append("--- errors ---")
        for e in J["errors_detail"]:
            L.append(f"  {e['file']} p{e['page']}: {e['note']}")
    (outdir / "structure-summary.txt").write_text("\n".join(L) + "\n", encoding="utf-8")
    print("\n".join(L))


if __name__ == "__main__":
    raise SystemExit(main())
