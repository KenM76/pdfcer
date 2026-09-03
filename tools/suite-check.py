#!/usr/bin/env python3
"""suite-check — turn the print-conformance suite into a pass/fail signal.

WHY THIS EXISTS
===============
The print-conformance suite ships 51 single-patch PDFs, each testing one
PDF/X feature. Its stated pass criterion is a human at 0.5 m looking for a
red X, which is not automatable — but the ARTWORK IS AUTHORED PRE-SWAPPED,
and that is what makes it mechanical. Each patch draws a trap X whose colour
is chosen so that a CORRECT renderer makes it vanish into its surround and
an INCORRECT one leaves it visible. Every patch states that criterion on its own
printed face, naming the feature under test and telling the reader that a
visible X means it rendered wrong. Those captions are the suite's own
copyrighted text and are NOT reproduced here -- operator ruling 2026-08-25;
see the private map directory named in this repository's scrub record.

So the pass/fail signal ships inside the corpus. No press, no proof, no
instrument, no reference measurement to source, and — importantly — no
second renderer to disagree with.

WHY NOT JUST DIFF AGAINST pdfium
================================
Because pdfium fails many of these tests too. It is a screen renderer with
no overprint, and `tools/render-parity` measured 11 "unexplained" and 40
"disclosed-gap" divergences across these same 51 patches — a number that
mixes pdfcer's failures with pdfium's and cannot separate them. The trap X is
an ORACLE; pdfium is a peer. Where an oracle exists, it wins (the same
argument `tools/check-image-colorspace-truth.py` makes for closed-form Lab).

HOW THE DETECTOR WORKS, AND WHY IT IS CONTENT-INDEPENDENT
=========================================================
The trap is two crossing DIAGONAL strokes. Essentially all other content on
these pages — text, table rules, swatch borders, the PDF/X-4 badge — is
AXIS-ALIGNED. So the discriminator is the ratio of diagonal to axis-aligned
edge energy in a sliding window:

    diag(x,y) = min(|dI/dx|, |dI/dy|)      both large  => a 45-degree edge
    axis(x,y) = | |dI/dx| - |dI/dy| |      one dominates => H or V edge
    score     = sum(diag) / sum(axis)

CALIBRATION, measured rather than chosen (2026-08-17, PCS 16.0 at scale 2.0,
the patch whose expected result was known independently because pdfcer had
just been changed to fix it):

    swatch        score     verdict
    Hue           0.650     X VISIBLE
    Saturation    0.599     X VISIBLE
    Color         0.570     X VISIBLE
    HardLight     0.043     clean
    Luminosity    0.026     clean
    Difference    0.019     clean
    Opacity 0%    0.015     clean
    Exclusion     0.014     clean

A 13x separation between the two populations, so the 0.25 threshold sits in
an empty gap rather than being tuned to a wanted answer (W14). The energy
floor exists only to reject small text glyphs, whose strokes are short
enough to produce a high ratio on very little total energy.

THE CONTRAST FLOOR WAS CALIBRATED ON ONE POPULATION (fixed 2026-08-24)
======================================================================
`CONTRAST_MIN` was 12.0, chosen against the ONLY population anyone had
measured: the sub-perceptual differences Acrobat leaves in a render that is,
to the eye, ten clean swatches. It rejected those correctly. It had never
been checked against a population of GENUINE traps of moderate contrast,
because none had been measured.

`PCS 1.0` is that population, and the operator read it cell by cell. At this
harness's own default scale its six X-shaped candidates measure:

    contrast   operator's reading
    10.7       cell i -- a clear fail
    10.2       cell d -- a clear fail
     7.8       cell j -- a clear fail
     7.4       cell e -- a clear fail
     4.1       cell b -- "a faint outline only ... the math for the edges
     4.1       cell g --  of the x differs slightly with rounding"

Four clear fails and two faint outlines, exactly as he described them, and
the two populations are separated by an EMPTY interval from 4.1 to 7.4. The
old floor of 12.0 sat above all six, so `PCS 1.0` reported `clean` while
carrying four crosses a human sees immediately.

6.0 sits inside that empty interval with ~1.4 levels of margin on each side.
The rest of the corpus was measured before it was chosen, not after:

  * the sub-perceptual population the old floor existed to reject is at or
    below 1.1 across every Acrobat render;
  * the "genuinely invisible" cells of `PCS 16.2`, which the operator agreed
    are invisible, are 1.5 to 3.3 -- below 4.1, so they stay rejected;
  * across all 51 patches the change flips exactly ONE verdict, `PCS 1.0`
    pass -> FAIL, which is the correction the operator asked for. Every
    other patch that gains a detection above 6.0 was already FAIL on a
    larger mark.

★ THE REVIEW'S DIAGNOSIS OF THIS FAULT WAS WRONG, and recording that matters
more than the fix. §3 of that document says the floor "has no area term" and
that box 3's crosses are "roughly three times the linear size" of the
calibration patch's, so the remedy is to make the threshold a function of
mark size. Measured here: at the scale this harness renders, every trap on
`PCS 1.0` is 36-38 px square and the `PCS 16.0` calibration traps are 38 px
square. **They are the same size.** An area term would have changed nothing
and the patch would have gone on reporting clean, with a fix in place and a
plausible reason to stop looking. The fault was never geometry; it was a
threshold calibrated against one population and applied to another.

THE POSITIVE CRITERION -- A MARK THAT SHOULD BE THERE
=====================================================
The suite marks failure two ways and this harness implements one. Beside the
cross that should vanish, four patches print a check mark that should be
PRESENT: *"If a check mark is visible in the upper right corner then DeviceN
is respected (= GOOD). If no check mark appears then DeviceN color was
transformed to CMYK (= ERROR)."*

A detector built to find a presence cannot see an absence. It does not report
"I cannot tell" -- it reports `clean`, which is indistinguishable from a pass,
and it had done so for these four for its entire life.

They are now reported as `MARK?` and counted as UNRESOLVED. That is not a
detector; it is the removal of a false green, and it is the more urgent half:
every "N of 51 pass" sentence this project has ever filed included four
patches the instrument had never examined.

★★★ THE GROUND TRUTH BELOW WAS WRONG ABOUT pdfcer ON THREE OF FOUR PATCHES,
AND IT IS CORRECTED HERE RATHER THAN REPLACED, BECAUSE THE ERROR IS THE
INSTRUCTIVE PART (re-measured 2026-08-27).

What this section said, recorded 2026-08-24:

  * `PCS 8.2` (PCS082): "Acrobat draws two OLIVE check marks ... pdfcer draws
    only the caption glyph. FAIL."
  * `PCS 8.01` (PCS080): "Acrobat draws two DARK-GREEN check marks on the
    images AND about fifteen more along the spot-colour gradient bar. pdfcer
    draws none of them. FAIL."
  * `PCS 8.1` (PCS081): "same family, same result. FAIL."
  * `PCS 5.0` (PCS050): the mark is a BLACK glyph from an embedded modified
    Symbol font. pdfcer renders it correctly. PASS.

Re-measured against the reference renders, side by side, at this harness's own
scale: **pdfcer draws both image check marks on PCS082, and on PCS080 it draws
both image marks AND all ~15 marks along the gradient bar**, in the right
colour and the right places. By each patch's own printed criterion -- "if a
check mark is visible then DeviceN is respected" -- pdfcer SATISFIES it.

What pdfcer actually misses on PCS080 is the **gradient bar behind the marks**,
which renders as bare white paper: 451 x 29 device pixels of missing
background, with the marks floating correctly on top of nothing. That is a
different defect, in a different subsystem (a `ShadingType 2` whose colour
space names two SPOT colorants, so ISO 32000-1 Table 149 under `/OP true`
preserves the whole backdrop and the bar paints nothing -- pdfcer has no spot
plane). The recorded sentence would have sent the next reader hunting for a
missing-glyph bug that does not exist.

⇒ **HOW THE ERROR HAPPENED IS WORTH MORE THAN THE CORRECTION.** The original
measurement was of the RIGHT THING -- "are the marks there?" -- taken from a
whole-page pixel diff against Acrobat. On PCS080 that diff is dominated by the
missing bar, and the marks sit inside the region it covers. "This region
differs enormously and the marks are in it" was read as "the marks are
missing". A large, real, correctly-detected difference **swallowed** a smaller
question asked about the same pixels.

★ AND THE ORIGINAL WARNING BELOW IS STILL RIGHT, which is why it is kept: the
mark's COLOUR is not a constant of the criterion. A detector keyed on one hue
passes PCS080 by matching the green end of its gradient bar. Whatever
adjudicates these must key on the mark's presence relative to a reference
render, not on a colour -- and, as the correction above shows, must key on the
MARK'S OWN PIXELS rather than on the region containing them.

WHAT A VERDICT HERE DOES AND DOES NOT MEAN
==========================================
`X` means the suite's own trap fired: that feature is not rendered
correctly. `clean` means no trap fired in that patch — which is the suite's
pass criterion and is NOT the same as "pixel-correct". A patch can be clean
and still differ from a press proof in ways the trap was not designed to
catch. This tool reports what the suite asks; it does not claim more.

USAGE
=====
    python tools/suite-check.py <dir-of-patch-pdfs> \
        --reference-dir <dir-of-reference-engine-renders> [--scale 2.0] [--json]

★★ PASS `--reference-dir`. IT IS NOT OPTIONAL IN PRACTICE AND IT WAS GOING
UNUSED. Without it, thirteen reference-strip patches are SCORED but not
ADJUDICATED and land in the UNRESOLVED bucket -- the harness has no calibration
for what "matching" scores on those layouts, so it honestly declines to
guess. With a directory of a known-good engine's renders of the SAME patches
(same file names, `.png`), that calibration exists: the reference engine sets
the score, and pdfcer is judged against it rather than against a number
somebody chose.

Measured 2026-08-27, same tree, same binary, only the flag differing:

    without --reference-dir     5 FAIL, 30 pass, 16 UNRESOLVED
    with    --reference-dir     5 FAIL, 35 pass, 11 UNRESOLVED

Five patches move from "the instrument cannot say" to a verdict, and nothing
else changes. The renders live beside the corpus -- the private map directory
names the environment variable that points at them.

★ It is also the only way to run the CONTROL that says whether a trap is
pdfcer's or the instrument's: point `find_traps` at the reference engine's own
render of a failing patch. Measured on the four failures of 2026-08-26, three
tripped ZERO traps in the reference render (so those are pdfcer's) and one
tripped two (so its count includes instrument noise and must not be read as
that many pdfcer defects).

Out-of-tree tooling, exactly like `tools/render-parity`: never shipped,
never in `cargo test`, never in the GUI-core `cargo tree` invariant.
Requires `pillow` + `numpy`; uses the shipped `pdfcer render-page`.
"""

import argparse
import json
import os
import subprocess
import sys

import cv2
import numpy as np

AREA_MIN = 200      # px; below this a mark is a glyph, not a swatch trap
# ★★ EDGE_MAX WAS 90 AND MISSED A REAL FAILURE BY FOUR PIXELS.
#
# The bound was calibrated on PCS 16.0, whose traps are 38x38 at the harness's
# default scale, with 90 as generous headroom. PCS 22.1's trap is **94x94** --
# a light X on a dark swatch, plainly visible to the operator, scoring 0.563
# fill and passing every OTHER test -- and it was rejected for being four
# pixels too wide. The harness reported that patch as a PASS.
#
# ⇒ A threshold calibrated on one specimen is a claim about every specimen. The
# fill and diagonal tests below are what actually discriminate a trap from a
# glyph or a swatch; the size bound was doing no work except excluding traps
# that happened to be large.
#
# 160 is chosen BY THE REFERENCE CONTROL rather than by taste. Run against all
# 51 Acrobat renders, which are the ground truth for "a correct engine trips
# nothing":
#
#   EDGE_MAX  90 / 120 / 160   Acrobat trips 2 patches (both already TRAP?)
#   EDGE_MAX  200              Acrobat trips a THIRD -- too loose
#
# So 160 is the largest value that does not start inventing failures in the
# engine being used as the oracle.
EDGE_MIN, EDGE_MAX = 16, 160
FILL_LO, FILL_HI = 0.15, 0.60
DIAG_MIN = 0.85
CONTRAST_MIN = 6.0    # 8-bit levels; below this the X is not "clear".
#                       12.0 until 2026-08-24 -- see THE CONTRAST FLOOR
#                       WAS CALIBRATED ON ONE POPULATION, in the docstring.

# Substrings that mean THIS PATCH SCORES ITSELF WITH A MARK THAT SHOULD BE
# PRESENT, rather than with a cross that should be absent. Matched against
# the patch's own extracted text, exactly as `ref_style` is -- the patch
# states its criterion on its face, so the harness reads it rather than
# carrying a hand-maintained list that can drift from the corpus.
#
# ★ FOUR PATCHES MATCH, NOT SEVEN. `docs/suite-operator-review-2026-08-21.md`
# §2 lists seven, from a grep of the ReadMes for "check mark", and three of
# them -- PCS150, PCS151, PCS152 -- are wrong. Those three say on their own
# face *"If a X can be seen, Optional Content is not handled right"*: the
# NEGATIVE criterion, which this harness already implements. Their ReadMes
# mention a check mark only while describing what the failure cross is drawn
# OUT OF ("a cross consisting of 2 check marks").
#
# ⇒ **a grep for a phrase finds a mention, not a criterion.** The list was
# built by searching for words and read as if it had been built by reading
# the rule, which is the same shape as every other false-green this harness
# has produced.
# ★★★ A STATED CRITERION OUTRANKS A MENTION -- the fourth classification
# defect in this harness, and the first that reported a PASS as a FAILURE.
#
# Five patches exercising 16-bit images caption their underlying 8-bit image
# "Reference image (cross): 8Bit, ZIP" while stating the X criterion outright:
# "No X must be visible when rendered correctly." The word "reference image"
# tripped `ref_style`, so they were routed to the strip comparator instead of
# the X detector -- and `content_bands` then correlated a 141-row photograph
# against the 21-row PAGE FOOTER, giving ~0.09 BY CONSTRUCTION.
#
# ★★ THE CONTROL THAT SETTLES IT: ACROBAT'S OWN RENDERS SCORE THE SAME.
# Run through this file's own `reference_similarity` at the same scale,
# Acrobat gives 0.094 / 0.065 / 0.109 against pdfcer's 0.089 / 0.057 / 0.098,
# and `None` for the same two patches. A metric that scores the oracle and the
# subject identically is not measuring the subject. The five renders are
# structurally identical to Acrobat's -- same size, position and orientation,
# no noise -- and tone-correlate 0.94-0.98, better than a patch this harness
# already passes.
#
# ★ AND THE TELL WAS SEEN AND MIS-READ ONCE ALREADY. This file's own comment
# further down records "four 16-bit-image patches 'passed' on scores of 0.05 vs
# 0.06 -- pdfcer agreeing with Acrobat that neither resembles the reference". A
# prior session spotted the anomaly, correctly refused to call it green, and
# added a guard DOWNSTREAM of the misclassification instead of asking why the
# number was absurd. The number was absurd because the patch was in the wrong
# comparator.
#
# This is `MARK_CRITERION`'s own warning turned on `ref_style`: a grep for a
# phrase finds a MENTION, not a criterion. So an explicit criterion wins.
X_CRITERION = ("no x must be visible", "no x should be visible",
               "no x may be visible")

MARK_CRITERION = ("check mark", "checkmark", "check marks", "checkmarks")

# ★★★ A THIRD CRITERION WITH NO DETECTOR, and the third false-green this
# harness has had to end.
#
# Some patches are scored neither by an absent cross nor by a present mark, but
# by comparing the live object against BAKED artwork printed beside it and
# labelled "correct" and "wrong". Others print an "expected result" row under
# the actual one. Neither draws a trap, so `find_traps` returns 0 and the patch
# scored `clean` BY DEFAULT -- the detector answering a question the patch never
# asked.
#
# ★ THIS WAS NOT A THEORY. The operator reported that some patches "show check
# boxes" while their numbers are wrong, and one of them -- PCS 3.1 -- is a real
# rendering failure that this harness has been reporting as `clean`. The
# CONTROL that settles it: `find_traps` on ACROBAT's own render of that patch
# also returns 0. A detector that gives a correct renderer and an incorrect one
# the same answer is not scoring them.
#
# ★★ THIS IS A MENTION-GREP, which is exactly what `MARK_CRITERION`'s note
# above warns against -- so it is used ONLY to say "cannot judge", NEVER to say
# "fail". A mention wrongly promoted to UNRESOLVED costs a line of output; a
# mention wrongly promoted to FAIL would be this harness inventing defects.
#
# Requiring BOTH words rather than either is what keeps it narrow. Measured
# over the corpus: it selects exactly three patches, moves exactly three
# verdicts, and changes nothing else. All three were then checked BY EYE rather
# than trusted from the grep -- one genuinely uses labelled correct/wrong
# artwork, two use a different expected-result design. They do not share one
# layout, which is why the verdict is the neutral `CRIT?` rather than a name
# that would claim they do.
CRITERION_UNKNOWN = (("correct", "wrong"),)


def cli_path():
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    for cand in ("pdfcer.exe", "pdfcer"):
        p = os.path.join(root, "target", "release", cand)
        if os.path.exists(p):
            return p
    sys.exit("build the release CLI first: cargo build --release -p pdfcer-cli")


def find_traps(png):
    """Locate trap X marks by EXACT intensity level and shape.

    Segmenting on exact levels rather than a quantised or thresholded image
    is what makes this work, and it is not an optimisation. The trap is a
    FLAT-FILLED shape drawn in one colour over a flat swatch of another —
    measured on PCS 16.0, a grey `178` X on a black `0` square — so the two
    are perfectly separable by value and the X falls out as one connected
    component. An edge detector sees only its outline (which is a hollow X,
    with none of the shape statistics below), and a quantiser can split two
    nearby trap colours into different buckets: `178` and `165` land in
    different bins at any step coarser than 13, and those are the actual
    values of two adjacent traps on the same patch.

    Shape test, all four measured on known traps before being fixed as
    thresholds (PCS 16.0: three traps, each 38x38, fill 0.44, diag 1.00;
    every clean swatch scored below 0.05 on the diagonal measure):

      * bbox 16..90 px square-ish -- a swatch-sized mark, not a glyph;
      * fill 0.15..0.60 -- an X is thin; a filled square or blob is not;
      * >=85% of the mark's pixels lie within 0.25 of a bbox diagonal;
      * >=200 px so a small letter cannot qualify on ratio alone.
    """
    im = cv2.imread(png, cv2.IMREAD_GRAYSCALE)
    if im is None:
        return []
    found = []
    levels, counts = np.unique(im, return_counts=True)
    for v in levels[counts >= AREA_MIN]:
        mask = (im == v).astype(np.uint8)
        n, lab, stats, _ = cv2.connectedComponentsWithStats(mask, 8)
        for i in range(1, n):
            x, y, w, h, area = (int(z) for z in stats[i])
            if area < AREA_MIN:
                continue
            if not (EDGE_MIN <= w <= EDGE_MAX and EDGE_MIN <= h <= EDGE_MAX):
                continue
            if abs(w - h) > max(w, h) * 0.4:
                continue
            fill = area / float(w * h)
            if not (FILL_LO <= fill <= FILL_HI):
                continue
            m = lab[y:y + h, x:x + w] == i
            yy, xx = np.nonzero(m)
            u = xx / max(w - 1, 1)
            vv = yy / max(h - 1, 1)
            d1 = np.abs(u - vv) < 0.25          # top-left -> bottom-right
            d2 = np.abs(u + vv - 1) < 0.25      # top-right -> bottom-left
            diag = float((d1 | d2).mean())
            if diag < DIAG_MIN:
                continue
            # ★ BOTH diagonals must carry real mass. Without this a SINGLE
            # diagonal stroke scores 1.00 -- every one of its pixels is
            # "near a diagonal" -- and the detector reports an X wherever a
            # slash, a chart rule or an anti-aliased corner appears. That
            # false positive is not hypothetical: it put 8 phantom traps on
            # an Acrobat render of PCS 2.0 whose ten swatches are provably
            # clean, and it inflated pdfcer's own failure count too. An X has
            # two arms; requiring each to hold at least a quarter of the
            # mark is what makes it an X rather than a line.
            if float(d1.mean()) < 0.25 or float(d2.mean()) < 0.25:
                continue
            # ★ AND THE ARMS MUST ACTUALLY CROSS. A real X has mass at the
            # centre of its bounding box, where the two strokes meet. A
            # hollow ring, an anti-aliased corner, or two opposite corner
            # wedges all satisfy "both diagonals carry mass" while having
            # nothing in the middle -- which is what still fired on
            # Acrobat's anti-aliased screen renders after the both-arms
            # constraint. Cheap, and it is the difference between "shaped
            # like a cross" and "shaped like anything on two diagonals".
            cy0, cy1 = int(h * 0.40), int(h * 0.60) + 1
            cx0, cx1 = int(w * 0.40), int(w * 0.60) + 1
            centre = m[cy0:cy1, cx0:cx1]
            if centre.size == 0 or centre.mean() < 0.55:
                continue
            # ★ AND IT MUST BE A *CLEAR* X. The suite's own wording is "a
            # clear X indicates the improper handling of a file", judged by
            # a human at 0.5 m -- so a mark that is geometrically an X but
            # only a shade away from its surround is a PASS by the suite's
            # criterion even though it is present in the pixels.
            #
            # This is not a convenience threshold. Segmenting on exact
            # intensity found genuine X marks in all eight swatches of an
            # Acrobat render of PCS 2.0 that is, to the eye, ten clean green
            # squares: Acrobat leaves a sub-perceptual difference. Counting
            # those made the detector STRICTER than the standard it is
            # implementing, which is its own kind of wrong answer.
            band = im[y:y + h, x:x + w]
            inside = float(band[m].mean())
            outside_mask = ~m
            if outside_mask.sum() < 20:
                continue
            outside = float(band[outside_mask].mean())
            if abs(inside - outside) < CONTRAST_MIN:
                continue
            found.append((x, y, w, h, round(fill, 2), round(diag, 2)))
    found.sort(key=lambda t: -t[2] * t[3])
    keep = []
    for f in found:
        if any(abs(f[0] - k[0]) < 20 and abs(f[1] - k[1]) < 20 for k in keep):
            continue
        keep.append(f)
    return keep



def content_bands(im):
    """Horizontal content bands separated by full-width white gaps."""
    ink = (im < 245).sum(axis=1)
    # 0.10, not 0.25. An "actual vs reference" patch whose rows are five
    # small images separated by white gutters never reaches a quarter of the
    # page width in ink, so a 0.25 gate found ONE band and the comparison
    # silently returned "unscorable" for seven of the eleven -- which reads
    # as a tooling limit and is really a threshold picked for a text-heavy
    # layout and then applied to an image-heavy one.
    rows = ink > im.shape[1] * 0.10
    segs, start = [], None
    for i, r in enumerate(rows):
        if r and start is None:
            start = i
        elif not r and start is not None:
            if i - start > 20:
                segs.append((start, i))
            start = None
    if start is not None and len(rows) - start > 20:
        segs.append((start, len(rows)))
    return segs


def classify(txt):
    """Which pass criterion does this patch state on its own face?

    Returns `(ref_style, mark_style, crit_style)`. `txt` is the patch's own
    extracted text, lowercased.

    ★ **Precedence is the whole point.** `ref_style` is a MENTION-grep, and a
    patch that merely mentions a reference image while stating the X criterion
    outright is an X-trap patch. Routing it to the strip comparator scored a
    photograph against the page footer and reported five passing patches as
    failures -- see `X_CRITERION`. So a stated criterion outranks a mention,
    and that ordering is the thing this function exists to make testable.

    The three flags are not mutually exclusive by construction, and the caller
    resolves them in its own documented order (X marks first, then `ref_style`,
    then `mark_style`, then `crit_style`, then `clean`).
    """
    ref = ("reference image" in txt) or ("match the reference" in txt)
    if any(k in txt for k in X_CRITERION):
        # The patch says what a failure looks like. Believe it over a caption.
        ref = False
    mark = any(k in txt for k in MARK_CRITERION)
    # All words in any one group must appear -- see CRITERION_UNKNOWN.
    crit = any(all(k in txt for k in g) for g in CRITERION_UNKNOWN)
    return ref, mark, crit


def self_test() -> int:
    """Pin the classification rules without needing a PDF or the corpus.

    The bug this guards was pure string classification, so the test is too --
    which also means it runs on a machine that has no licensed corpus at all.
    """
    # ★ THE REGRESSION. A verbatim lowercased transcript of what `extract-text`
    # returns for a 16-bit patch: it mentions a reference image AND states the
    # X criterion. Pinning the real string rather than a paraphrase is the
    # point -- a paraphrase would have passed while the real one failed.
    t = ("no x must be visible when rendered correctly. "
         "reference image (cross): 8bit, zip test image: 16 bit, zip")
    assert classify(t)[0] is False, "a stated X criterion must outrank a mention"

    # Unchanged: genuine reference-strip patches, which state no X criterion.
    assert classify("each of these should match the reference images")[0] is True
    assert classify("reference image")[0] is True

    # The other two criteria still resolve, and are independent of the above.
    assert classify("if a check mark is visible then devicen is respected")[1] is True
    assert classify("the correct result is on the left and the wrong one on the right")[2] is True
    assert classify("nothing in particular is stated here") == (False, False, False)

    # ★★ THE METRIC MUST FALL WHEN THE RENDER GETS WORSE, which is exactly
    # what its predecessor did not do: destroying every shading RAISED
    # `reference_similarity` by +0.130 / +0.349. Pinned synthetically, in
    # memory, so this runs with no PDF, no corpus and no licensed file.
    import tempfile

    def _write(img):
        fd, path = tempfile.mkstemp(suffix=".png")
        os.close(fd)
        cv2.imwrite(path, img)
        return path

    h, w = 120, 200
    good = np.zeros((h, w), np.uint8)
    good[:, :] = np.linspace(0, 255, w).astype(np.uint8)[None, :]
    cv2.circle(good, (w // 2, h // 2), 30, 0, -1)
    same = good.copy()
    # The ablation: the artwork destroyed, the page otherwise identical.
    broken = good.copy()
    cv2.rectangle(broken, (0, 0), (w, h), 0, -1)
    # A different page entirely -- the far end of the scale.
    other = np.full((h, w), 255, np.uint8)
    cv2.circle(other, (30, 30), 20, 0, -1)

    ref, a, b, c = _write(good), _write(same), _write(broken), _write(other)
    try:
        identical = engine_similarity(a, ref)[0]
        destroyed = engine_similarity(b, ref)[0]
        unrelated = engine_similarity(c, ref)[0]
        assert identical > 0.99, f"an identical render must score ~1.0, got {identical}"
        assert destroyed < identical, (
            f"destroying the artwork must LOWER the score -- the metric this "
            f"replaced raised it. identical={identical}, destroyed={destroyed}"
        )
        assert unrelated < ENGINE_CORR_MIN, (
            f"an unrelated page must fall below the threshold, got {unrelated}"
        )
    finally:
        for path in (ref, a, b, c):
            os.unlink(path)
    print("suite-check --self-test: the engine metric falls when the render worsens")

    print("suite-check --self-test: classification rules hold")
    return 0


# ★★★ NOT A NUMBER SOMEBODY LIKED. Calibrated by ablation on the two shading
# patches: a CORRECT render scores 0.822 / 0.783 against the reference engine,
# and the SAME render with every shading destroyed scores 0.573 - 0.771. 0.75
# sits inside that gap.
#
# It is a FLOOR for "the artwork is structurally there", not a claim of pixel
# equality -- two engines never reach 1.0 on a text-heavy page, because they
# hint and antialias type differently.
ENGINE_CORR_MIN = 0.75


def engine_similarity(png, ref_png, grid=160):
    """Correlate a render against a KNOWN-GOOD ENGINE'S render of the same page.

    ★★★ WHY THIS EXISTS, AND IT IS WORSE THAN A BAD THRESHOLD.
    `reference_similarity` compares a render **to itself** -- its "objects"
    band against its "reference images" band. On a patch laid out as a 2x2
    GRID of (object, reference-image) pairs, `content_bands` finds the two GRID
    ROWS, and the comparison correlates row 1's artwork against row 2's
    ENTIRELY DIFFERENT artwork. The resulting number is a function of how much
    the two rows happen to resemble each other, and NOT of whether anything
    rendered correctly.

    ★★ MEASURED BY ABLATION, and the direction is the point: replacing every
    shading in pdfcer's own render with SOLID BLACK **RAISES**
    `reference_similarity` from 0.823 -> 0.953 and 0.445 -> 0.794. Rotating
    every shading 180 degrees moves it by +0.000 / -0.003. The old metric is
    ANTI-CORRELATED with shading fidelity on this layout -- a renderer that
    painted nothing where every shading belongs scores BETTER than the correct
    one, and one that draws them upside-down is indistinguishable.

    The same ablations run through THIS function all move the right way
    (-0.006 .. -0.212), because it compares two renders of the same page.

    ★ Both images are reduced to a fixed `grid`-wide luminance raster first.
    The two engines hint and antialias type differently, and at full resolution
    that noise dominates a page whose ink is mostly text. Downsampling averages
    it away and leaves the artwork, which is what these patches actually test.

    Returns `(correlation, mean_abs_diff)`, or `None` if either image is
    unreadable -- which is a genuine "cannot tell", not a failure.
    """
    a = cv2.imread(ref_png, cv2.IMREAD_GRAYSCALE)
    b = cv2.imread(png, cv2.IMREAD_GRAYSCALE)
    if a is None or b is None:
        return None
    h = max(8, round(grid * a.shape[0] / a.shape[1]))
    a = cv2.resize(a, (grid, h), interpolation=cv2.INTER_AREA).astype(np.float32)
    b = cv2.resize(b, (grid, h), interpolation=cv2.INTER_AREA).astype(np.float32)
    corr = float(((a - a.mean()) * (b - b.mean())).mean() / (a.std() * b.std() + 1e-6))
    return corr, float(np.abs(a - b).mean())


def reference_similarity(png):
    """Compare a patch's "Actual test objects" strip to its "Reference
    Images" strip.

    ★ THIS EXISTS BECAUSE THE X-TRAP DETECTOR SILENTLY PASSED PATCHES THAT
    VISIBLY FAIL. The suite uses (at least) TWO evaluation designs, and only
    one of them draws an X. Thirteen of the 51 patches instead print the
    test objects above a strip of REFERENCE IMAGES and say "each of these
    ... should match the reference images". On those, "no X found" is not a
    pass — it is the detector answering a question the patch never asked.
    `PCS 16.10` is the case that exposed it: it reported clean while two of
    its five reference cells rendered as empty boxes.

    Returns (correlation, mean-abs-difference). A correct render makes the
    two strips near-identical; the labels row differs by construction, which
    is why this reports a SCORE rather than a verdict — the threshold is not
    yet calibrated against a known-passing patch, and inventing one would be
    exactly the W14 error this harness's sibling was built to avoid.
    """
    im = cv2.imread(png, cv2.IMREAD_GRAYSCALE)
    if im is None:
        return None
    segs = content_bands(im)
    if len(segs) < 2:
        return None
    segs = sorted(segs, key=lambda t: -(t[1] - t[0]))[:2]
    segs.sort()
    (a0, a1), (b0, b1) = segs
    a = cv2.resize(im[a0:a1], (im.shape[1], min(a1 - a0, b1 - b0))).astype(np.float32)
    b = cv2.resize(im[b0:b1], (im.shape[1], min(a1 - a0, b1 - b0))).astype(np.float32)
    corr = float(((a - a.mean()) * (b - b.mean())).mean() / (a.std() * b.std() + 1e-6))
    return corr, float(np.abs(a - b).mean())


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("dir")
    ap.add_argument("--scale", type=float, default=2.0)
    ap.add_argument("--json", action="store_true")
    ap.add_argument(
        "--reference-dir",
        help="directory of renders from a KNOWN-GOOD engine, same filenames. "
             "Used only to adjudicate the reference-strip patches, which "
             "carry no trap X and cannot otherwise be judged.",
    )
    args = ap.parse_args()

    cli = cli_path()
    tmp = os.path.join(args.dir, "_render")
    os.makedirs(tmp, exist_ok=True)

    pdfs = sorted(f for f in os.listdir(args.dir) if f.lower().endswith(".pdf"))
    results = []
    for f in pdfs:
        png = os.path.join(tmp, f + ".png")
        proc = subprocess.run(
            [cli, "render-page", os.path.join(args.dir, f), "--page", "1",
             "--scale", str(args.scale), "-o", png],
            capture_output=True,
        )
        if proc.returncode != 0 or not os.path.exists(png):
            results.append({"patch": f, "verdict": "RENDER-FAILED", "traps": 0})
            continue
        marks = find_traps(png)
        # Does this patch use the reference-strip design rather than an X?
        txt = subprocess.run([cli, "extract-text", os.path.join(args.dir, f)],
                             capture_output=True, text=True, errors="replace").stdout.lower()
        ref_style, mark_style, crit_style = classify(txt)
        sim = reference_similarity(png) if ref_style else None
        ref_sim = None
        eng = None
        if marks and mark_style:
            # ★★ A MARK-CRITERION PATCH IS NEVER SCORED BY THE CROSS DETECTOR
            # (2026-09-02, `Pass 239.0`). A check mark IS two diagonal strokes,
            # so on a patch whose pass condition is "green check marks appear"
            # the diagonal-energy detector fires on the very marks that mean
            # success. Measured on PCS 8.1 after its spot planes landed: pdfcer's
            # render trips 4 "traps" -- all of them check marks and the
            # duotone's fin edge -- while the reference engine's render of the
            # same patch trips 0 at a slightly different contrast, and the two
            # renders are indistinguishable by eye. Before this Pass the same
            # patch DID draw real crosses and the detector counted them, which
            # is why the FAIL looked earned: the instrument was right for the
            # wrong reason, then wrong for the same reason.
            #
            # So: the count is reported, and the verdict is the one this
            # harness already uses for a criterion it cannot detect. NOT a
            # pass -- the operator still has to look -- and NOT a FAIL, because
            # a diagonal stroke on this patch is not evidence of anything.
            verdict = "MARK?"
        elif marks:
            verdict = "X"
            # THE SAME HONESTY GUARD THE STRIP COMPARISON ALREADY HAS, applied
            # to the trap detector, which had none.
            #
            # A trap is an X-shaped mark that a CORRECT render makes invisible.
            # The whole verdict rests on the premise that a correct engine
            # trips zero of them -- so if the REFERENCE engine's own render
            # trips traps on this patch, that premise is false HERE and the
            # detector is not discriminating between a good render and a bad
            # one. It is reading the patch's ordinary artwork as a mark.
            #
            # ★ Measured, 2026-09-01, across all six patches this harness was
            # calling FAIL:
            #
            #   patch      pdfcer traps    reference traps
            #   PCS 2.0        7               0
            #   PCS 3.0        3               0
            #   PCS 4.0        3               0
            #   PCS 8.1        1               0
            #   PCS 13.0       2               0
            #   PCS 16.1       1               2      <-- reference trips MORE
            #
            # Five of the six are genuine: the reference is clean and pdfcer is
            # not. The sixth is this harness inventing a defect -- pdfcer trips
            # FEWER marks there than the engine being used as ground truth,
            # and was still scored FAIL while the reference would have scored
            # worse.
            #
            # ⇒ The verdict becomes TRAP?, not pass. "The instrument cannot
            # say" is the honest answer and is deliberately NOT a pass: the
            # patch may still be rendering wrongly for reasons this detector
            # cannot see. Promoting it to `clean` would repeat the exact error
            # `CRIT?` and `MARK?` were introduced to end.
            if args.reference_dir:
                rcand = os.path.join(args.reference_dir, f.replace(".pdf", "") + ".png")
                if not os.path.exists(rcand):
                    rcand = os.path.join(args.reference_dir, f + ".png")
                if os.path.exists(rcand) and find_traps(rcand):
                    verdict = "TRAP?"
        elif ref_style:
            verdict = "REF"          # scored, not adjudicated -- see docstring
            # With a known-good engine's render of the SAME patch, the strip
            # comparison becomes adjudicable: the reference engine sets what
            # "matching" scores on this layout, and pdfcer is judged against
            # that rather than against a number somebody chose.
            if args.reference_dir:
                cand = os.path.join(args.reference_dir, f.replace(".pdf", "") + ".png")
                if not os.path.exists(cand):
                    cand = os.path.join(args.reference_dir, f + ".png")
                if os.path.exists(cand):
                    ref_sim = reference_similarity(cand)
                # ★ THE GUARD THAT MAKES THIS HONEST: if the reference
                # engine does not match its OWN embedded strip, the band
                # split is wrong for this layout and the comparison
                # measures nothing. Say so instead of scoring it. Without
                # this, four 16-bit-image patches "passed" on scores of
                # 0.05 vs 0.06 -- pdfcer agreeing with Acrobat that neither
                # resembles the reference, read as success.
                # ★★ THE ORACLE IS THE OTHER ENGINE'S RENDER, NOT THIS
                # ONE'S OTHER HALF. `ref_sim` -- the reference engine's score
                # against its own embedded strip -- is KEPT, but only as the
                # honesty guard it was written to be: it says whether the band
                # split suits this layout. It never adjudicates again, because
                # a layout it does NOT suit can still score high by accident.
                # PCS 6.0 scores 0.817 on it and passes a guard set at 0.50,
                # on a 2x2 grid the band split cannot read at all -- so
                # `REF-PASS` was printed for a number measuring nothing.
                eng = engine_similarity(png, cand)
                if eng is not None:
                    verdict = "REF-PASS" if eng[0] >= ENGINE_CORR_MIN else "REF-FAIL"
                elif ref_sim is not None and sim is not None and ref_sim[0] >= 0.50:
                    verdict = "REF-PASS" if sim[0] >= ref_sim[0] - 0.05 else "REF-FAIL"
        elif mark_style:
            # ★ NOT `clean`. This patch is scored by a mark that should be
            # PRESENT, and nothing here looks for one. Reporting `clean`
            # would be the detector answering a question the patch never
            # asked -- the same error `REF` was introduced to end, one
            # criterion over.
            verdict = "MARK?"
        elif crit_style:
            # ★ NOT `clean`. This patch states a criterion nothing here
            # detects, so the honest answer is "cannot judge". Reporting a
            # pass because the detector found nothing is how PCS 3.1 -- a
            # real rendering failure -- sat behind a green tick.
            verdict = "CRIT?"
        else:
            verdict = "clean"
        results.append({
            "patch": f,
            "verdict": verdict,
            "traps": len(marks),
            "where": [f"{m[0]},{m[1]}" for m in marks[:6]],
            "ref_corr": None if sim is None else round(sim[0], 3),
            "ref_absdiff": None if sim is None else round(sim[1], 1),
            "ref_engine_corr": None if ref_sim is None else round(ref_sim[0], 3),
            "engine_corr": None if eng is None else round(eng[0], 3),
            "engine_absdiff": None if eng is None else round(eng[1], 1),
        })

    if args.json:
        print(json.dumps(results, indent=2))
        return 0

    clean = [r for r in results if r["verdict"] in ("clean", "REF-PASS")]
    failed = [r for r in results if r["verdict"] in ("X", "REF-FAIL")]
    ref = [r for r in results if r["verdict"] in ("REF", "MARK?", "CRIT?", "TRAP?")]
    broke = [r for r in results if r["verdict"] == "RENDER-FAILED"]
    for r in results:
        mark = {"clean": "  ok  ", "X": " FAIL ", "REF": " ref? ",
                "REF-PASS": "  ok  ", "REF-FAIL": " FAIL ",
                "MARK?": " mark?", "CRIT?": " crit?", "TRAP?": " trap?",
                "RENDER-FAILED": " ERR  "}[r["verdict"]]
        if r["verdict"] == "X":
            extra = f"  {r['traps']} trap(s) at {' '.join(r['where'])}"
        elif r["verdict"] == "TRAP?":
            extra = (f"  {r['traps']} trap(s) detected, but the REFERENCE engine's "
                     "own render trips traps here too, so the detector is reading "
                     "ordinary artwork as a mark on this layout and cannot "
                     "discriminate; unjudged rather than FAIL (and NOT a pass)")
        elif r["verdict"] == "MARK?":
            extra = ("  scored by a check mark that should be PRESENT; "
                     "this harness only detects marks that should be ABSENT")
        elif r["verdict"] == "CRIT?":
            extra = ("  states a criterion this harness has NO detector for "
                     "(baked correct/wrong artwork, or an expected-result row); "
                     "reported unjudged rather than clean")
        elif r["verdict"].startswith("REF"):
            extra = (f"  strip corr={r['ref_corr']}"
                     + (f" vs reference-engine {r['ref_engine_corr']}"
                        if r.get("ref_engine_corr") is not None else ""))
        else:
            extra = ""
        print(f"{mark} {r['patch']}{extra}")
    print()
    print(f"suite-check: {len(results)} patches -- "
          f"{len(failed)} FAIL, "
          f"{len(clean)} pass, "
          f"{len(ref)} UNRESOLVED (reference-strip, positive-criterion, "
          f"no detector, or a trap the reference trips too), "
          f"{len(broke)} render errors")
    print()
    print("A 'clean' verdict is the SUITE's own pass criterion for an X-trap")
    print("patch and is NOT a claim of pixel-accuracy against a press proof.")
    print("A 'mark?' row is NOT a pass either: that patch is scored by a")
    print("check mark that should be PRESENT, and this harness has no")
    print("detector for an absent mark. Ground truth for those four is in")
    print("the docstring; three of them are known failures.")
    print("A 'ref?' row is NOT a pass: those patches carry their reference")
    print("images inline and are scored, not adjudicated, because no")
    print("known-passing patch exists yet to calibrate a threshold against.")
    print("A correlation well below 1.0 means the strips visibly differ.")
    print("A 'trap?' row is NOT a pass and NOT a failure: this harness")
    print("detected trap marks, but the REFERENCE engine's own render trips")
    print("traps on the same patch, so the detector is reading ordinary")
    print("artwork as a mark there and cannot tell a good render from a bad")
    print("one. The patch may still be wrong for reasons nothing here sees.")
    return 0



if "--self-test" in sys.argv:
    raise SystemExit(self_test())

if __name__ == "__main__":
    raise SystemExit(main())
