#!/usr/bin/env python3
"""compare.py — pdfcer-vs-pdfium divergence on a CMYK page, decision 006 §3.7's method.

WHY THIS EXISTS
---------------
Decision 006 §3.7 measured pdfcer against pdfium on one 300x232 DeviceCMYK JPEG
at 1:1 and reported the numbers that made the colorimetry gap a tracked item:

    max abs delta per channel   [11, 37, 30]
    95th percentile per channel [ 5, 27, 18]
    mean abs delta per channel  [2.47, 9.61, 6.82]
    pixels differing > 8 in some channel   37.4 %

That last figure is the one the project quotes. Any claim that the conversion
improved has to be measured the SAME way on the SAME page, or it is an
assertion rather than a result. This script is that measurement, written down
so it is reproducible instead of being a throwaway probe (the same reasoning
that turned decision 006's own survey scripts into committed fixtures).

METHOD (identical to 006 §3.7, restated so it can be rebuilt from this note)
---------------------------------------------------------------------------
1. Render the page in pdfcer via the built `pdfcer render-page --scale 1`.
   Scale 1 means one PDF unit per device pixel; the cmyk-variants fixtures
   place their image at 1:1, so one image sample lands on one pixel and no
   resampling allowance is needed.
2. Render the same page in pdfium via `pypdfium2` at the same scale.
3. Composite both onto white so transparency cannot differ, crop to the common
   top-left extent, and take the per-channel absolute difference.
4. Report max / p95 / mean per channel, and the fraction of pixels where SOME
   channel differs by more than 8, 16 and 32 out of 255.

The default page is `fixtures/synthetic/cmyk-variants/v2.pdf`, which wraps the
exact codestream 006 measured (see that directory's PROVENANCE.md).

USAGE
-----
    python compare.py                       # the decision 006 page
    python compare.py --pdf some/other.pdf --page 1 --scale 1
"""

from __future__ import annotations

import argparse
import subprocess
import sys
import tempfile
from pathlib import Path

import numpy as np

REPO = Path(__file__).resolve().parents[2]
DEFAULT_PDF = REPO / "fixtures" / "synthetic" / "cmyk-variants" / "v2.pdf"


def on_white(img):
    """Composite RGBA onto white; return an HxWx3 uint8 array.

    Both engines are asked for the same page, but they need not agree on how
    they represent "nothing painted here". Compositing onto white normalizes
    that so a transparency difference cannot masquerade as a colour difference.
    """
    from PIL import Image

    if img.mode != "RGBA":
        img = img.convert("RGBA")
    bg = Image.new("RGBA", img.size, (255, 255, 255, 255))
    return np.asarray(Image.alpha_composite(bg, img).convert("RGB"), dtype=np.int16)


def render_pdfce(pdf: Path, page: int, scale: float):
    from PIL import Image

    exe = REPO / "target" / "release" / ("pdfcer.exe" if sys.platform == "win32" else "pdfcer")
    if not exe.exists():
        raise SystemExit(f"build it first: cargo build --release -p pdfcer-cli ({exe} missing)")
    out = Path(tempfile.mkdtemp(prefix="cmyk-cmp-")) / "pdfcer.png"
    subprocess.run(
        [str(exe), "render-page", "--page", str(page), "--scale", str(scale), "-o", str(out), str(pdf)],
        check=True,
        capture_output=True,
    )
    img = Image.open(out)
    img.load()
    return img


def render_pdfium(pdf: Path, page: int, scale: float):
    import pypdfium2 as pdfium

    doc = pdfium.PdfDocument(str(pdf))
    return doc[page - 1].render(scale=scale, draw_annots=False).to_pil()


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--pdf", type=Path, default=DEFAULT_PDF)
    ap.add_argument("--page", type=int, default=1)
    ap.add_argument("--scale", type=float, default=1.0)
    ap.add_argument("--label", default="", help="tag the output line (e.g. before/after)")
    args = ap.parse_args()

    a = on_white(render_pdfce(args.pdf, args.page, args.scale))
    b = on_white(render_pdfium(args.pdf, args.page, args.scale))
    h = min(a.shape[0], b.shape[0])
    w = min(a.shape[1], b.shape[1])
    if a.shape[:2] != b.shape[:2]:
        print(f"note: sizes differ {a.shape[:2]} vs {b.shape[:2]}; cropping to {(h, w)}")
    d = np.abs(a[:h, :w] - b[:h, :w])
    chan_max = d.max(axis=2)

    tag = f" [{args.label}]" if args.label else ""
    # ASCII only: this runs at a Windows console whose default code page is
    # cp1252, where a lone em dash prints as a replacement character.
    print(f"{args.pdf.name} page {args.page} @ scale {args.scale} - {h}x{w} px{tag}")
    print(f"  max abs delta per channel   {list(d.max(axis=(0, 1)))}")
    print(f"  95th percentile per channel {[round(float(np.percentile(d[:, :, i], 95)), 2) for i in range(3)]}")
    print(f"  mean abs delta per channel  {[round(float(d[:, :, i].mean()), 2) for i in range(3)]}")
    for t in (8, 16, 32):
        print(f"  pixels differing > {t:<2} in some channel   {(chan_max > t).mean() * 100:.2f} %")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
