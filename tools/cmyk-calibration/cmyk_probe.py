#!/usr/bin/env python3
"""cmyk_probe.py — measure a renderer's DeviceCMYK -> sRGB transfer function.

WHY THIS EXISTS
---------------
ISO 32000-1 §8.6.4.4 defines DeviceCMYK as "four components, each 0.0 (zero
concentration) to 1.0 (maximum concentration), subtractive" and **mandates no
conversion formula whatsoever**. The colour a DeviceCMYK operand produces on an
RGB display is therefore entirely implementation-defined. There is no spec text
to be correct against — only other implementations to agree or disagree with.

That makes this a *measurement* problem, not a spec-reading problem. To improve
pdfcer's conversion you first need ground truth: what sRGB does a production
renderer actually put on screen for a given (c,m,y,k)? This script produces
that, by the only method that cannot be wrong about it — rendering known CMYK
patches and reading the pixels back.

WHAT IT DOES
------------
1. Emits a synthetic PDF (`--pdf`) containing one solid-filled rectangle per
   lattice point of the CMYK unit hypercube, laid out on a square grid. Each
   patch is `PATCH` PDF units square and is painted with `c m y k k` + `re f`,
   i.e. the DeviceCMYK non-stroking operator — no images, no ICC, no shading,
   nothing that could route the colour through a different code path than a
   plain vector fill.
2. Renders that PDF at scale 1.0 with pdfium (via `pypdfium2`) and/or pdfcer
   (via the built `pdfcer render-page`), which makes one PDF unit exactly one
   device pixel.
3. Samples the CENTRE pixel of each patch — never an edge — so anti-aliasing on
   the patch boundary cannot contaminate the reading.
4. Writes a TSV: `c m y k  r g b` with c,m,y,k in [0,1] and r,g,b in 0..255.

The lattice is deliberately parameterised (`--levels`) so a FIT set and a
disjoint VALIDATION set can be generated (e.g. `--levels 9` to fit,
`--levels 6` to validate: 9-level and 6-level lattices share only the points
where i/8 == j/5, i.e. 0 and 1, so a 6-level check is almost entirely
out-of-sample).

Output is deterministic and locale-invariant: fixed lattice order, fixed
formatting, no clocks.

USAGE
-----
    python cmyk_probe.py --levels 9 --engine pdfium --out fit-pdfium.tsv
    python cmyk_probe.py --levels 9 --engine pdfcer  --out fit-pdfce.tsv
    python cmyk_probe.py --levels 6 --engine pdfium --out val-pdfium.tsv

`--engine pdfcer` shells out to `target/release/pdfcer`, exactly like
`tools/render-parity` does; this script imports nothing from pdfcer and pdfcer
depends on nothing here (LEGAL §6 — pypdfium2 is tooling-only, never a pdfcer
runtime dependency).
"""

from __future__ import annotations

import argparse
import math
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]

# Patch size in PDF units == device pixels at scale 1.0. 6 is comfortably wide
# enough that the centre pixel is >=2px from any anti-aliased edge.
PATCH = 6


def lattice(levels: int) -> list[tuple[float, float, float, float]]:
    """The sampled CMYK points, in a fixed (c, m, y, k) row-major order.

    `levels` values per axis, evenly spaced over the closed interval [0, 1]
    including both endpoints — the endpoints matter most (pure inks and paper
    white are where a bad conversion is most visible), so a lattice that omits
    them would measure the easy interior and miss the hard corners.
    """
    if levels < 2:
        raise ValueError("need at least 2 levels so 0.0 and 1.0 are both sampled")
    axis = [i / (levels - 1) for i in range(levels)]
    return [(c, m, y, k) for c in axis for m in axis for y in axis for k in axis]


def build_pdf(points, path: Path) -> tuple[int, int]:
    """Write the patch-grid PDF. Returns (columns, page_side_in_units).

    The grid is as square as possible so the page stays within sane dimensions;
    with 9^4 = 6561 patches at 6 units that is an 81x81 grid on a 486x486 page.
    """
    cols = math.ceil(math.sqrt(len(points)))
    rows = math.ceil(len(points) / cols)
    side_w = cols * PATCH
    side_h = rows * PATCH

    ops = []
    for idx, (c, m, y, k) in enumerate(points):
        col = idx % cols
        row = idx // cols
        x = col * PATCH
        # PDF y grows upward; lay row 0 at the TOP so the raster row index and
        # the lattice index run the same direction (one fewer sign to get wrong).
        y0 = side_h - (row + 1) * PATCH
        ops.append(f"{c:.6f} {m:.6f} {y:.6f} {k:.6f} k {x} {y0} {PATCH} {PATCH} re f")
    content = "\n".join(ops).encode("ascii")

    objs: list[bytes] = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        (
            f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {side_w} {side_h}] "
            f"/Resources << >> /Contents 4 0 R >>"
        ).encode("ascii"),
        b"<< /Length " + str(len(content)).encode("ascii") + b" >>\nstream\n" + content + b"\nendstream",
    ]

    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets = []
    for i, body in enumerate(objs, start=1):
        offsets.append(len(out))
        out += f"{i} 0 obj\n".encode("ascii") + body + b"\nendobj\n"
    xref_at = len(out)
    out += f"xref\n0 {len(objs) + 1}\n".encode("ascii")
    out += b"0000000000 65535 f \n"
    for off in offsets:
        out += f"{off:010d} 00000 n \n".encode("ascii")
    out += (
        f"trailer\n<< /Size {len(objs) + 1} /Root 1 0 R >>\nstartxref\n{xref_at}\n".encode("ascii")
        + b"%%EOF\n"
    )
    path.write_bytes(bytes(out))
    return cols, side_h


def sample(image, points, cols: int, side_h: int):
    """Read the centre pixel of every patch out of a rendered RGB array."""
    import numpy as np

    arr = np.asarray(image.convert("RGB"), dtype=np.uint8)
    got = []
    for idx in range(len(points)):
        col = idx % cols
        row = idx // cols
        px = col * PATCH + PATCH // 2
        py = row * PATCH + PATCH // 2
        got.append(tuple(int(v) for v in arr[py, px]))
    return got


def build_cost_fixture(path: Path, side: int = 2000) -> None:
    """Emit the performance fixture described under `--emit-cost-fixture`.

    The sample field is `(x * 7 + y * 13) % 256` over the interleaved CMYK
    bytes: cheap to generate, fully deterministic (so two timing runs measure
    the same work), and spatially incoherent enough that consecutive pixels
    land in different cells of the conversion's node grid. A gradient or a flat
    fill would let the cache — and any future memoisation — hide the cost that
    is being measured.
    """
    import zlib

    rows = bytearray()
    for y in range(side):
        rows += bytes((x * 7 + y * 13) % 256 for x in range(side * 4))
    data = zlib.compress(bytes(rows), 1)
    content = f"q {side} 0 0 {side} 0 0 cm /Im0 Do Q".encode("ascii")
    objs = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        (
            f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {side} {side}] "
            f"/Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>"
        ).encode("ascii"),
        b"<< /Length " + str(len(content)).encode("ascii") + b" >>\nstream\n" + content + b"\nendstream",
        (
            f"<< /Type /XObject /Subtype /Image /Width {side} /Height {side} "
            f"/ColorSpace /DeviceCMYK /BitsPerComponent 8 /Filter /FlateDecode "
            f"/Length {len(data)} >>"
        ).encode("ascii")
        + b"\nstream\n"
        + data
        + b"\nendstream",
    ]
    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets = []
    for i, body in enumerate(objs, start=1):
        offsets.append(len(out))
        out += f"{i} 0 obj\n".encode("ascii") + body + b"\nendobj\n"
    xref_at = len(out)
    out += f"xref\n0 {len(objs) + 1}\n".encode("ascii") + b"0000000000 65535 f \n"
    for off in offsets:
        out += f"{off:010d} 00000 n \n".encode("ascii")
    out += (
        f"trailer\n<< /Size {len(objs) + 1} /Root 1 0 R >>\nstartxref\n{xref_at}\n".encode("ascii")
        + b"%%EOF\n"
    )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(bytes(out))


def render_pdfium(pdf: Path):
    import pypdfium2 as pdfium

    doc = pdfium.PdfDocument(str(pdf))
    page = doc[0]
    bitmap = page.render(scale=1.0, draw_annots=False)
    return bitmap.to_pil()


def render_pdfce(pdf: Path):
    from PIL import Image

    exe = REPO / "target" / "release" / ("pdfcer.exe" if sys.platform == "win32" else "pdfcer")
    if not exe.exists():
        raise SystemExit(f"build it first: cargo build --release -p pdfcer-cli  ({exe} missing)")
    png = pdf.with_suffix(".png")
    subprocess.run(
        [str(exe), "render-page", "--page", "1", "--scale", "1", "-o", str(png), str(pdf)],
        check=True,
        capture_output=True,
    )
    img = Image.open(png)
    img.load()
    return img


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--levels", type=int, default=9, help="samples per CMYK axis (default 9)")
    ap.add_argument(
        "--random",
        type=int,
        default=0,
        help=(
            "instead of a lattice, sample N uniformly-random CMYK points (seeded, "
            "so the set is reproducible). This is the HONEST out-of-sample check: a "
            "lattice validation set silently coincides with the model's own grid "
            "nodes whenever the two resolutions share a divisor, which turns "
            "validation into interpolation-of-known-points and flatters the fit."
        ),
    )
    ap.add_argument("--seed", type=int, default=20260808, help="seed for --random")
    ap.add_argument(
        "--emit-cost-fixture",
        type=Path,
        help=(
            "write the PERFORMANCE fixture instead of probing: a single-page PDF "
            "holding one 2000x2000 DeviceCMYK image (4 megapixels, 16 MB of "
            "samples, FlateDecode'd) whose samples are a deterministic "
            "pseudo-random field. Deliberately INCOHERENT: it is the worst case "
            "for cache locality and defeats any run-length or memoisation "
            "shortcut, so the render time it produces is an upper bound on the "
            "conversion's per-pixel cost rather than a friendly average. Time it "
            "with `pdfcer render-page --scale 1`; see README section 6."
        ),
    )
    ap.add_argument("--engine", choices=("pdfium", "pdfcer"), default="pdfium")
    # Not `required=True`: `--emit-cost-fixture` writes a PDF and produces no
    # TSV, and argparse would otherwise demand an output path for a mode that
    # has no output to name.
    ap.add_argument("--out", type=Path, help="TSV output path (required unless --emit-cost-fixture)")
    ap.add_argument("--pdf", type=Path, help="keep the generated patch PDF here")
    args = ap.parse_args()

    if args.emit_cost_fixture:
        build_cost_fixture(args.emit_cost_fixture)
        print(f"wrote cost fixture to {args.emit_cost_fixture}")
        return 0

    if args.out is None:
        raise SystemExit("--out is required unless --emit-cost-fixture is given")

    if args.random:
        import numpy as np

        rng = np.random.default_rng(args.seed)
        points = [tuple(float(v) for v in row) for row in rng.random((args.random, 4))]
    else:
        points = lattice(args.levels)
    tmpdir = tempfile.mkdtemp(prefix="cmyk-probe-")
    pdf = args.pdf if args.pdf else Path(tmpdir) / "patches.pdf"
    pdf.parent.mkdir(parents=True, exist_ok=True)
    cols, side_h = build_pdf(points, pdf)

    img = render_pdfium(pdf) if args.engine == "pdfium" else render_pdfce(pdf)
    if img.size != (cols * PATCH, side_h):
        print(f"warning: render size {img.size} != expected {(cols * PATCH, side_h)}", file=sys.stderr)
    rgb = sample(img, points, cols, side_h)

    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("w", encoding="utf-8", newline="\n") as fh:
        fh.write("# engine\t%s\n# levels\t%d\n" % (args.engine, args.levels))
        fh.write("c\tm\ty\tk\tr\tg\tb\n")
        for (c, m, y, k), (r, g, b) in zip(points, rgb):
            fh.write(f"{c:.6f}\t{m:.6f}\t{y:.6f}\t{k:.6f}\t{r}\t{g}\t{b}\n")
    print(f"wrote {len(points)} samples to {args.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
