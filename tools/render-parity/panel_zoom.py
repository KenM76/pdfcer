#!/usr/bin/env python3
"""panel_zoom — crop a `benign_structure.py` 4-panel image around its finding.

WHY THIS EXISTS
===============
`benign_structure.py` writes full-page panels laid out as

    [ pdfcer | pdfium | 8x delta | classification ]

At 125 DPI a Letter page is 1063x1375, so a full panel is ~4276 px wide. The
finding that matters is often a few hundred pixels across, and at
whole-panel scale it is invisible -- which would leave the audit in exactly
the position it is auditing: a number nobody can check by looking.

This crops the SAME page-space rectangle out of all four sub-panels, stacks
them vertically (so the eye compares along one axis instead of across a
4,000 px span), and nearest-neighbour upscales. Nearest, not Lanczos: the
question is usually "is this pixel painted or not", and a smoothing resample
invents intermediate values that make a 1 px hairline look like a soft edge.

The crop rectangle comes from `structure.tsv`'s `off_lcc_bbox` column (the
bounding box of the largest unexplained contiguous region), padded, so the
tool always frames the thing that was flagged rather than a guess.

USAGE
=====
    python panel_zoom.py --tsv out-benign-audit/structure.tsv --rank 0
    python panel_zoom.py --tsv out-benign-audit/structure.tsv \
        --file external/pdfium/testing/resources/bookmarks.pdf --page 1
    python panel_zoom.py ... --top 12        # first 12 ranked rows, one file each

Writes `<panel-dir>/zoom/<name>.png` and prints the paths.
"""

from __future__ import annotations

import argparse
import csv
from pathlib import Path

from PIL import Image

HERE = Path(__file__).resolve().parent
PANEL_GAP = 8  # must match classification_panel()'s separator width


def panel_for(row: dict, panels_dir: Path) -> Path | None:
    """Find the emitted panel for a row by its filename convention."""
    stem = row["rel"].replace("/", "_").replace("\\", "_")
    for ch in '<>:"|?*':
        stem = stem.replace(ch, "_")
    tail = f"_{stem}_p{row['page']}.png"
    for p in sorted(panels_dir.glob("*.png")):
        if p.name.endswith(tail[:190]):
            return p
    return None


def zoom(panel: Path, bbox: str, pad: int, scale: int, out: Path) -> None:
    im = Image.open(panel)
    pw = (im.width - 3 * PANEL_GAP) // 4
    if bbox:
        x0, y0, x1, y1 = (int(v) for v in bbox.split(","))
    else:  # no unexplained region: frame the whole page
        x0, y0, x1, y1 = 0, 0, pw - 1, im.height - 1
    x0 = max(0, x0 - pad); y0 = max(0, y0 - pad)
    x1 = min(pw - 1, x1 + pad); y1 = min(im.height - 1, y1 + pad)
    w, h = x1 - x0 + 1, y1 - y0 + 1
    crops = [im.crop((i * (pw + PANEL_GAP) + x0, y0,
                      i * (pw + PANEL_GAP) + x0 + w, y0 + h)) for i in range(4)]
    canvas = Image.new("RGB", (w, h * 4 + 3 * 6), (120, 120, 120))
    for i, c in enumerate(crops):
        canvas.paste(c, (0, i * (h + 6)))
    if scale > 1:
        canvas = canvas.resize((canvas.width * scale, canvas.height * scale),
                               Image.NEAREST)
    out.parent.mkdir(parents=True, exist_ok=True)
    canvas.save(out)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--tsv", default="out-benign-audit/structure.tsv")
    ap.add_argument("--panels", default="")
    ap.add_argument("--rank", type=int, default=-1)
    ap.add_argument("--top", type=int, default=0)
    ap.add_argument("--file", default="")
    ap.add_argument("--page", type=int, default=1)
    ap.add_argument("--pad", type=int, default=30)
    ap.add_argument("--scale", type=int, default=3)
    args = ap.parse_args()

    tsv = Path(args.tsv)
    if not tsv.is_absolute():
        tsv = HERE / tsv
    panels = Path(args.panels) if args.panels else tsv.parent / "panels"
    with tsv.open(encoding="utf-8", newline="") as fh:
        rows = list(csv.DictReader(fh, delimiter="\t"))

    if args.file:
        sel = [r for r in rows if r["rel"] == args.file and int(r["page"]) == args.page]
    elif args.top:
        sel = rows[: args.top]
    elif args.rank >= 0:
        sel = rows[args.rank: args.rank + 1]
    else:
        sel = rows[:1]

    for r in sel:
        p = panel_for(r, panels)
        if p is None:
            print(f"  (no panel emitted) {r['rel']} p{r['page']}")
            continue
        out = panels / "zoom" / f"z{int(r['off_lcc_px']):07d}_{p.name}"
        zoom(p, r["off_lcc_bbox"], args.pad, args.scale, out)
        print(f"{out}   off_lcc={r['off_lcc_px']} bbox={r['off_lcc_bbox']} "
              f"sharedEdge={float(r['shared_edge_frac']):.3f} {r['rel']} p{r['page']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
