#!/usr/bin/env python3
"""tolerated_scan — count the disclosure `render_parity.py` cannot see.

WHY
===
`render_parity.py` decides whether a page is "clean-by-construction" from
`parse_diag_line`, which reads ONLY the machine-readable `k=v` tally that
`pdfcer render-page` prints on its first stdout line, and then keeps only
the keys listed in `GAP_KEYS`.

`pdfcer` also prints a SECOND line, in prose, when the interpreter had to
tolerate something it could not act on:

    pdfcer: note: 2 structural oddity(ies) tolerated while interpreting the page

That counter (`Diagnostics::tolerated`) is not in the `k=v` line at all, so
the harness never parses it, and it is not in `GAP_KEYS`, so it could not
count as a disclosed gap even if it were. A page can therefore emit
"I could not act on 2 things on this page" and still be recorded `clean=1`,
enter the clean-by-construction population, and help DEFINE the tolerance
band that decides which pages are benign.

That is not hypothetical. `pdfium/testing/resources/multiple_graphics_states.pdf`
prints exactly the note above -- both its `gs` operators no-op because
`apply_ext_gstate` does not resolve an indirect `/ExtGState` entry -- and the
baseline records it `clean=1`, bucket `benign`.

This tool measures how large that blind spot is: for every page the baseline
bucketed, it re-runs `render-page`, captures the tolerated count, and reports
how many pages disclose a tolerated oddity while the harness recorded them as
clean. It renders pdfcer ONLY (no reference renderer, no image maths), so it
costs a fraction of a parity sweep.

OUTPUT
======
  <outdir>/tolerated.tsv   file, page, bucket, clean(from baseline), tolerated
  printed summary          the cross-tabulation that matters:
                           tolerated>0 AND clean==1, by bucket.
"""

from __future__ import annotations

import argparse
import csv
import re
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import render_parity as rp

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent

# The prose note pdfcer prints when Diagnostics::tolerated > 0. Matching on
# the leading integer keeps this robust to wording changes after "oddity".
NOTE_RE = re.compile(r"note:\s*(\d+)\s+structural oddit", re.I)


def probe(rel: str, page: int, scale: float, corpus_root: Path, timeout: float) -> int:
    """Return the tolerated count for one page, or -1 if the render failed."""
    with tempfile.TemporaryDirectory() as td:
        out = Path(td) / "p.png"
        cmd = [str(rp.CLI), "render-page", str(corpus_root / rel),
               "--page", str(page), "--scale", f"{scale:.6f}",
               "--no-annotations", "-o", str(out)]
        try:
            r = subprocess.run(cmd, capture_output=True, timeout=timeout)
        except subprocess.TimeoutExpired:
            return -1
        if r.returncode != 0:
            return -1
        # The note is printed on STDERR, not stdout. That is precisely why
        # `render_parity.py` cannot see it: `render_pdfce` parses `r.stdout`
        # only, and reads `r.stderr` solely to build an error message on a
        # non-zero exit. Search both streams here so the measurement does not
        # depend on which one pdfcer happens to use.
        blob = r.stdout.decode(errors="replace") + "\n" + r.stderr.decode(errors="replace")
        m = NOTE_RE.search(blob)
        return int(m.group(1)) if m else 0


def main(argv: list[str] | None = None) -> int:
    for s in (sys.stdout, sys.stderr):
        if hasattr(s, "reconfigure"):
            s.reconfigure(encoding="utf-8", errors="replace")
    ap = argparse.ArgumentParser()
    ap.add_argument("--tsv", default="out-corpus-4023/per-page.tsv")
    ap.add_argument("--outdir", default="out-benign-audit")
    ap.add_argument("--corpus-root", default=str(ROOT / "fixtures"))
    ap.add_argument("--dpi", type=float, default=125.0)
    ap.add_argument("--bucket", default="", help="'' = every ok row")
    ap.add_argument("--jobs", type=int, default=4)
    ap.add_argument("--timeout", type=float, default=60.0)
    args = ap.parse_args(argv)

    tsv = Path(args.tsv)
    if not tsv.is_absolute():
        tsv = HERE / tsv
    outdir = Path(args.outdir)
    if not outdir.is_absolute():
        outdir = HERE / outdir
    outdir.mkdir(parents=True, exist_ok=True)
    corpus_root = Path(args.corpus_root)
    scale = args.dpi / 72.0

    with tsv.open(encoding="utf-8", newline="") as fh:
        rows = [r for r in csv.DictReader(fh, delimiter="\t") if r["status"] == "ok"]
    if args.bucket:
        rows = [r for r in rows if r["bucket"] == args.bucket]

    def work(r):
        return r, probe(r["file"], int(r["page"]), scale, corpus_root, args.timeout)

    out = []
    with ThreadPoolExecutor(max_workers=args.jobs) as ex:
        for i, (r, t) in enumerate(ex.map(work, rows)):
            out.append((r, t))
            if (i + 1) % 500 == 0:
                print(f"  [{i+1}/{len(rows)}]", flush=True)

    with (outdir / "tolerated.tsv").open("w", encoding="utf-8", newline="\n") as fh:
        fh.write("file\tpage\tbucket\tclean\tgaps\ttolerated\n")
        for r, t in out:
            fh.write(f"{r['file']}\t{r['page']}\t{r['bucket']}\t{r['clean']}\t"
                     f"{r['gaps']}\t{t}\n")

    tot = len(out)
    tol = [(r, t) for r, t in out if t > 0]
    tol_clean = [(r, t) for r, t in tol if r["clean"] == "1"]
    tol_clean_benign = [(r, t) for r, t in tol_clean if r["bucket"] == "benign"]
    fail = [(r, t) for r, t in out if t < 0]
    print()
    print("=== tolerated-oddity disclosure the parity harness never reads ===")
    print(f"pages probed                                   : {tot}")
    print(f"  render failed now (excluded from ratios)     : {len(fail)}")
    print(f"pages with tolerated > 0                       : {len(tol)}"
          f"  ({100.0*len(tol)/max(1,tot):.1f}%)")
    print(f"  ...AND recorded clean-by-construction        : {len(tol_clean)}")
    print(f"  ...AND bucketed 'benign-renderer-noise'      : {len(tol_clean_benign)}")
    print()
    print("A clean-by-construction page is one the band is DERIVED from. Every")
    print("page in the third line above told pdfcer's operator that the")
    print("interpreter could not act on something, and still helped set the")
    print("threshold that decides which divergences are called benign.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
