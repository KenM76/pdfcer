#!/usr/bin/env python3
"""Score an OCR'd scan against the vector page it was made from.

WHY THIS EXISTS
---------------
`ROADMAP.md` recorded, at `Pass 71.0`'s ship: *"62 words returned is a COUNT,
not an accuracy result."* This is the thing that turns a count into a result.

★ IT SCORES TWO INDEPENDENT FAILURES, AND CONFLATING THEM IS THE WHOLE
PROBLEM WITH A WORD COUNT.

  * **CONTENT** - was the word read correctly? A recogniser that reads
    `lnvoice` for `Invoice` fails here.
  * **POSITION** - did the invisible word land on the ink? A recogniser can
    read every word perfectly and place the layer mirrored, offset or scaled,
    and the page still LOOKS perfect because the layer is invisible. **This is
    the failure an operator actually reports** - *"the OCR text does not line
    up with the image"* - and no count can see it.

A mirrored text layer scores **100 %** on content.

HOW THE GROUND TRUTH IS OBTAINED, and why it is trustworthy
------------------------------------------------------------
Not by transcription. `fixtures/synthetic/ocr/printed.pdf` is the VECTOR page
the scan was rendered from, so where a word "really is" comes from Helvetica's
own AFM metrics through pdfcer's already-tested extraction path. The two
rectangles being compared therefore arrive by **completely different routes** -
font metrics on one side, a neural recogniser looking at pixels on the other -
which is what makes their agreement a result rather than a tautology (`R215`:
never assert against a blessed copy of your own output).

WHAT IT PRINTS
--------------
    ocr-accuracy words_truth=N found=N missing=N content_pct=NN.N
                 median_offset_pt=N.NN p95_offset_pt=N.NN max_offset_pt=N.NN
                 within_2pt=NN.N within_5pt=NN.N

`content_pct` is recall over DISTINCT truth words. Offsets are the distance
between rectangle CENTRES, in points; 1 pt is 1/72 inch, so 2 pt is about the
height of a full stop in 12 pt text and is a tight bar for a recogniser working
on a degraded raster.

★ MEDIAN AND P95, NOT MEAN. A mean is dragged by a single wild outlier and
would hide a layer that is correct for 95 % of words and catastrophic for the
rest; the median says what a typical word does, and p95 says how bad the tail
gets. Both matter and they fail differently.

USAGE
-----
    python tools/ocr-accuracy.py <ocred.pdf>
    python tools/ocr-accuracy.py <ocred.pdf> --truth <GROUND_TRUTH.json>

EXIT CODES
----------
    0  scored (whatever the score) - scoring is not a pass/fail gate
    1  could not run: a missing file, no CLI, an empty truth set
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_TRUTH = ROOT / "fixtures" / "synthetic" / "ocr" / "GROUND_TRUTH.json"


def cli() -> Path:
    for rel in ("target/release/pdfcer.exe", "target/debug/pdfcer.exe",
                "target/release/pdfcer", "target/debug/pdfcer"):
        p = ROOT / rel
        if p.exists():
            return p
    sys.exit("pdfcer not built")


def find_rects(pdf: Path, needle: str) -> list[list[float]]:
    """Every hit for `needle`, as `[llx, lly, urx, ury]`.

    Uses `find-text`, i.e. the same operator-facing verb somebody would use to
    check the result by hand. Measuring through the shipped path rather than a
    private one means this cannot score a capability the operator does not
    have.
    """
    r = subprocess.run(
        [str(cli()), "find-text", str(pdf), "--needle", needle],
        capture_output=True, text=True, encoding="utf-8",
    )
    out = []
    for ln in (r.stdout + r.stderr).splitlines():
        if ln.startswith("match ") and "rect=" in ln:
            out.append([float(v) for v in ln.split("rect=", 1)[1].split()[0].split(",")])
    return out


def centre(rect: list[float]) -> tuple[float, float]:
    return ((rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("ocred", type=Path, help="the PDF with the OCR layer")
    ap.add_argument("--truth", type=Path, default=DEFAULT_TRUTH)
    ap.add_argument("--verbose", action="store_true",
                    help="print every word, its offset, and every miss")
    args = ap.parse_args()

    if not args.ocred.exists():
        sys.exit(f"no such file: {args.ocred}")
    truth = json.loads(args.truth.read_text(encoding="utf-8"))
    words: dict[str, list[list[float]]] = truth["words"]
    if not words:
        sys.exit("the ground truth carries no words")

    offsets: list[float] = []
    missing: list[str] = []
    for wtok, truth_rects in sorted(words.items()):
        got = find_rects(args.ocred, wtok)
        if not got:
            missing.append(wtok)
            if args.verbose:
                print(f"MISS  {wtok}")
            continue
        # ★ Best pairing, not first-against-first. A word occurring twice on
        # the page has two truth rects and may have two hits; scoring them in
        # list order would manufacture a large offset out of nothing but
        # ordering, on exactly the commonest words. The nearest pair is the
        # only defensible reading of "did this word land where it belongs".
        best = min(
            (
                (abs(centre(g)[0] - centre(t)[0]) ** 2 + abs(centre(g)[1] - centre(t)[1]) ** 2) ** 0.5
                for g in got
                for t in truth_rects
            ),
        )
        offsets.append(best)
        if args.verbose:
            print(f"ok    {wtok:<16} offset={best:6.2f} pt")

    for wtok in missing:
        if not args.verbose:
            break
    found = len(offsets)
    total = len(words)
    offsets.sort()

    def pct(vals: list[float], q: float) -> float:
        if not vals:
            return float("nan")
        i = min(int(q * (len(vals) - 1) + 0.5), len(vals) - 1)
        return vals[i]

    within = lambda lim: (100.0 * sum(1 for o in offsets if o <= lim) / found) if found else 0.0

    print(
        f"ocr-accuracy words_truth={total} found={found} missing={len(missing)} "
        f"content_pct={100.0 * found / total:.1f} "
        f"median_offset_pt={pct(offsets, 0.5):.2f} "
        f"p95_offset_pt={pct(offsets, 0.95):.2f} "
        f"max_offset_pt={offsets[-1] if offsets else float('nan'):.2f} "
        f"within_2pt={within(2.0):.1f} within_5pt={within(5.0):.1f}"
    )
    if missing:
        # Named, not just counted. "Which words" is what tells a reader whether
        # the recogniser has a systematic weakness or simply smudged one line.
        print("missing: " + " ".join(missing))


if __name__ == "__main__":
    main()
