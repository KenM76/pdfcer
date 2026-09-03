#!/usr/bin/env python3
"""corpus_cmyk.py — aggregate pdfcer-vs-pdfium divergence over every corpus page
that actually contains DeviceCMYK.

WHY THIS EXISTS, AND WHY IT IS NOT `tools/render-parity`
--------------------------------------------------------
`render-parity` is the standing full-corpus render-fidelity gate and it stays
the authority on *regressions*. But it is the wrong instrument for sizing a
colour change, for two reasons:

1. Its page metric is `frac_over_32` — the fraction of pixels differing by more
   than 32/255 — deliberately chosen to be blind to anything smaller so that
   anti-aliasing noise cannot masquerade as a bug. The DeviceCMYK colorimetry
   gap lived almost entirely BETWEEN 8/255 and 32/255 (measured: 37.40 % of
   pixels over 8, but only 0.04 % over 32 on decision 006's page). A metric
   built to ignore that band cannot measure closing it.
2. It walks the whole corpus, of which DeviceCMYK pages are a few dozen, so the
   signal is diluted by thousands of pages the change cannot affect.

So this script does the narrow thing: byte-scan the corpus for `/DeviceCMYK`,
render only those files in both engines, and report the >8/255 statistic
decision 006 §3.7 established — per file and pooled.

The byte-scan is the same detection `render-parity` uses for its DeviceCMYK
characterization bucket (README §6): presence of the literal `/DeviceCMYK`,
with no render-side counter added, because observing is not applying.

USAGE
-----
    cargo build --release -p pdfcer-cli
    python corpus_cmyk.py --label after
    python corpus_cmyk.py --label after --list out/cmyk-files.txt

Skips are counted and reported by reason, never silently dropped: some corpus
files are deliberately broken (conformance `fail-*` cases), and at least one
pdfium test resource CRASHES pdfium itself, so each render runs in a child
process and a dead child is a skip rather than the end of the run.
"""

from __future__ import annotations

import argparse
import pathlib
import subprocess
import sys
import tempfile

import numpy as np

REPO = pathlib.Path(__file__).resolve().parents[2]
CORPUS = REPO / "fixtures" / "external"
PDFCER = REPO / "target" / "release" / ("pdfcer.exe" if sys.platform == "win32" else "pdfcer")

# Render pdfium in a CHILD process. `fixtures/external/pdfium/testing/resources/
# bug_457855936.pdf` aborts pdfium outright (exit 0x80000003, STATUS_BREAKPOINT
# — a pdfium internal CHECK), which is exactly what a file named after a bug
# report is for. In-process that abort takes the whole sweep down with no
# traceback and no partial results.
PDFIUM_CHILD = (
    "import sys, pypdfium2 as p;"
    "d = p.PdfDocument(sys.argv[1]);"
    "d[0].render(scale=float(sys.argv[3]), draw_annots=False).to_pil().save(sys.argv[2])"
)


def find_cmyk_files(root: pathlib.Path):
    """Byte-scan for the literal `/DeviceCMYK`, sorted for determinism."""
    out = []
    for p in sorted(root.rglob("*.pdf")):
        try:
            if b"/DeviceCMYK" in p.read_bytes():
                out.append(p)
        except OSError:
            continue
    return out


def on_white(path: pathlib.Path):
    from PIL import Image

    img = Image.open(path)
    img.load()
    if img.mode != "RGBA":
        img = img.convert("RGBA")
    bg = Image.new("RGBA", img.size, (255, 255, 255, 255))
    return np.asarray(Image.alpha_composite(bg, img).convert("RGB"), dtype=np.int16)


def measure(pdf: pathlib.Path, scale: float, tmp: pathlib.Path):
    """Return (frac>8, frac>16, frac>32, pixels) or a skip reason string."""
    a_png, b_png = tmp / "a.png", tmp / "b.png"
    for f in (a_png, b_png):
        f.unlink(missing_ok=True)

    r = subprocess.run(
        [str(PDFCER), "render-page", "--page", "1", "--scale", str(scale), "-o", str(a_png), str(pdf)],
        capture_output=True,
        timeout=120,
    )
    if r.returncode != 0 or not a_png.exists():
        return "pdfcer"
    r = subprocess.run(
        [sys.executable, "-c", PDFIUM_CHILD, str(pdf), str(b_png), str(scale)],
        capture_output=True,
        timeout=120,
    )
    if r.returncode != 0 or not b_png.exists():
        return "pdfium"

    a, b = on_white(a_png), on_white(b_png)
    h, w = min(a.shape[0], b.shape[0]), min(a.shape[1], b.shape[1])
    if h == 0 or w == 0:
        return "empty"
    d = np.abs(a[:h, :w] - b[:h, :w]).max(axis=2)
    n = d.size
    return (float((d > 8).sum()) / n, float((d > 16).sum()) / n, float((d > 32).sum()) / n, n)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--scale", type=float, default=1.7361, help="125 DPI, the render-parity baseline")
    ap.add_argument("--label", default="")
    ap.add_argument("--list", type=pathlib.Path, help="newline-separated file list (skips the byte-scan)")
    ap.add_argument("--per-file", action="store_true", help="print every file, worst first")
    ap.add_argument(
        "--tsv",
        type=pathlib.Path,
        help=(
            "also write per-file fractions here. Two runs' TSVs can be joined to "
            "answer the question a pooled number cannot: how many INDIVIDUAL pages "
            "improved, how many were untouched, and — the one that matters — "
            "whether any got worse."
        ),
    )
    args = ap.parse_args()

    if not PDFCER.exists():
        raise SystemExit(f"build it first: cargo build --release -p pdfcer-cli ({PDFCER} missing)")

    files = (
        [pathlib.Path(l) for l in args.list.read_text(encoding="utf-8").split("\n") if l.strip()]
        if args.list
        else find_cmyk_files(CORPUS)
    )
    tmp = pathlib.Path(tempfile.mkdtemp(prefix="corpus-cmyk-"))

    rows, skips = [], {}
    for f in files:
        got = measure(f, args.scale, tmp)
        if isinstance(got, str):
            skips[got] = skips.get(got, 0) + 1
            continue
        rows.append((got[0], got[1], got[2], got[3], f))

    tag = f" [{args.label}]" if args.label else ""
    print(f"DeviceCMYK corpus sweep{tag}: {len(files)} files, {len(rows)} measured, {sum(skips.values())} skipped")
    for reason, n in sorted(skips.items()):
        print(f"  skipped ({reason}): {n}")
    if not rows:
        return 1

    px = sum(r[3] for r in rows)
    # Pooled = pixel-weighted, so a big page counts for what it is; the mean of
    # per-page fractions would let a postage-stamp page outvote a poster.
    for i, t in enumerate((8, 16, 32)):
        pooled = sum(r[i] * r[3] for r in rows) / px
        per_page = float(np.mean([r[i] for r in rows]))
        print(f"  pixels > {t:<2}/255: pooled {pooled * 100:6.2f} %   mean-per-page {per_page * 100:6.2f} %")
    print(f"  pages with >1 % of pixels beyond 8/255: {sum(1 for r in rows if r[0] > 0.01)} / {len(rows)}")

    if args.tsv:
        args.tsv.parent.mkdir(parents=True, exist_ok=True)
        with args.tsv.open("w", encoding="utf-8", newline="\n") as fh:
            fh.write("frac_gt8\tfrac_gt16\tfrac_gt32\tpixels\tfile\n")
            for frac8, frac16, frac32, n, f in sorted(rows, key=lambda r: str(r[4])):
                fh.write(f"{frac8:.6f}\t{frac16:.6f}\t{frac32:.6f}\t{n}\t{f}\n")
        print(f"  per-file TSV: {args.tsv}")

    if args.per_file:
        print("\n  worst pages by fraction over 8/255:")
        for frac8, _, _, _, f in sorted(rows, reverse=True)[:20]:
            print(f"    {frac8 * 100:6.2f} %  {f}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
