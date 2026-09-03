#!/usr/bin/env python3
"""Closed-form ground truth for the CIE-based image colour spaces.

WHY THIS EXISTS — AND WHY IT OUTRANKS THE PARITY HARNESS HERE
=============================================================
`tools/render-parity` compares pdfcer against pdfium. Its own module docstring
is careful about what that is worth: pdfium is a CROSS-CHECK, not ground
truth. Where the two disagree, the harness can only report an
`unexplained-divergence` — it cannot say which renderer is wrong, because it
has no third opinion.

For `/Lab`, `/CalGray` and `/CalRGB` a third opinion is available and exact.
These three spaces are defined by CLOSED-FORM arithmetic in ISO 32000-1
8.6.5.2-8.6.5.4; combined with the sRGB encode of IEC 61966-2-1 there is a
single correct 8-bit answer for every input tuple, computable here in a few
lines of Python that share no code with either renderer. That makes this
script an independent oracle rather than a second opinion, and it is the
right tool whenever a divergence on those spaces has to be ATTRIBUTED rather
than merely counted.

It exists because it was needed: on 2026-08-17 `lab.pdf` was the single
`unexplained-divergence` in the image-fixture parity run (mean delta 15.175,
frac32 0.121, dmax 156), and the standing assumption -- recorded in the
session handoff -- was that pdfcer's uncalibrated XYZ->sRGB conversion was the
cause. Measured against this oracle, pdfcer is correct to within 1/255 on all
three spaces and PDFIUM is not — Lab mean 40.854 / max 152, CalGray 2.000 / 9,
CalRGB 3.012 / 9. The assumption had the direction of the error backwards.
See `ROADMAP.md`'s `Pass 85.x` entry.

(The Lab figures are from the ASYMMETRIC-/Range fixture. An earlier symmetric
`[-100 100 -100 100]` version measured pdfium at 14.532 / 115; it was replaced
because it could not detect an a/b range transposition — see the fixture
generator's own note. Quoting the old number against the current fixture would
be comparing two different measurements.)

WHAT IT DOES NOT COVER
======================
`/Separation`, `/DeviceN` and `/Indexed` are excluded on purpose: their
result depends on a tint transform sampled from the document (a PostScript
calculator, in the DeviceN case), so "ground truth" would mean
re-implementing pdfcer's function evaluator here and comparing it with
itself. That is a tautology, not an oracle. Those spaces stay with the
parity harness and with unit tests over the function evaluator.

`ICCBased` is likewise out of scope and always will be: a correct answer
needs a real colour-management engine. That is `D:\\Dev\\iccce\\`'s half of
the boundary, not pdfcer's.

USAGE
=====
    python tools/check-image-colorspace-truth.py <fixture-dir> [--json]

`<fixture-dir>` is a directory produced by
`tools/gen-image-colorspace-fixtures.py`. Requires `pypdfium2`, `pillow` and
`numpy`, exactly like the parity harness, and like it is out-of-tree tooling:
never shipped, never in `cargo test`, never in the GUI-core `cargo tree`
invariant, and never in `THIRD_PARTY_LICENSES.md`.

Exit code is 1 if pdfcer's own error against truth exceeds `--tol` (default 2,
i.e. two 8-bit codes, which absorbs rounding and the renderer's own f32
arithmetic) so it can be run as a gate. pdfium's error is REPORTED and never
gates: this script measures pdfcer, and pdfium's numbers are here only so
that a future divergence can be attributed at a glance.

SAMPLING — WHY THE INTERIOR ONLY
================================
The fixtures put a 64x64 image on a 128pt page rendered at 150 DPI (267px),
so one image texel is ~4.17 device pixels and the raster edge rows are only
partially covered. Each engine blends that partial coverage with the page
backdrop in its own way, which is genuine and benign resampling noise. A
2-texel border is therefore excluded and each texel is sampled at its
CENTRE, so what is compared is the colour conversion and not the resampler.
"""

import json
import os
import subprocess
import sys

import numpy as np
import pypdfium2 as pdfium
from PIL import Image

# The encoding guard seven sibling `check-*` scripts carry. This one prints
# only ASCII today, so it is LATENT rather than live -- and it is added
# anyway, on 2026-08-18, because the day's lesson was that repairing the
# instance in front of you is not repairing the class. The live instance was
# `check-core-api-verbs.py`, which emitted a cp1252 em-dash on every run of
# the day it was written; this file is the only other gate without the guard,
# and the cost of closing it now is one line against a future edit that adds
# an arrow to a failure message and quietly loses it.
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")

W = H = 64
PW = 128
DPI = 150
SCALE = DPI / 72.0
WP = (0.9505, 1.0, 1.089)
BORDER = 2

# sRGB (IEC 61966-2-1) XYZ->linear-RGB matrix, D65.
M = (
    (3.2406, -1.5372, -0.4986),
    (-0.9689, 1.8758, 0.0415),
    (0.0557, -0.2040, 1.0570),
)


def xyz_to_srgb(X, Y, Z):
    """XYZ (D65-relative) -> 8-bit sRGB, with per-channel clipping.

    Clipping is the only defensible choice without a colour-management
    engine and a rendering intent: an out-of-gamut colour has no sRGB
    representation, and clipping each channel independently is what every
    non-managed renderer does. It is disclosed rather than hidden -- the
    `img_uncalibrated=` counter on `render-page`'s result line reports that
    this path ran (project rule 4: fuzzy, never sneaky).
    """
    out = []
    for row in M:
        v = row[0] * X + row[1] * Y + row[2] * Z
        v = min(1.0, max(0.0, v))
        v = 12.92 * v if v <= 0.0031308 else 1.055 * v ** (1 / 2.4) - 0.055
        out.append(int(round(v * 255)))
    return tuple(out)


def lab_truth(x, y):
    """ISO 32000-1 8.6.5.4 Lab -> XYZ, with the Table 89 image Decode.

    The Decode default for a Lab IMAGE is `[0 100 amin amax bmin bmax]`
    taken from `/Range`, not `[0 1 ...]`. Getting this wrong produces a
    picture that still looks like a picture, which is why it is asserted
    here against an independent computation rather than eyeballed.
    """
    # Must mirror gen-image-colorspace-fixtures.py's `lab` fixture exactly:
    # /Range [-100 100 -60 60] (ASYMMETRIC on purpose -- see that file) and
    # b varying along the diagonal so no component is constant.
    L = x * 4 / 255.0 * 100.0
    a = -100.0 + y * 4 / 255.0 * 200.0
    b = -60.0 + ((x + y) * 2) / 255.0 * 120.0
    fy = (L + 16.0) / 116.0
    fx = fy + a / 500.0
    fz = fy - b / 200.0

    def g(t):
        return t ** 3 if t > 6.0 / 29.0 else (108.0 / 841.0) * (t - 4.0 / 29.0)

    return xyz_to_srgb(WP[0] * g(fx), WP[1] * g(fy), WP[2] * g(fz))


def calgray_truth(x, y):
    """ISO 32000-1 8.6.5.2: X = Xw*A^G, Y = Yw*A^G, Z = Zw*A^G."""
    a = (x * 4 / 255.0) ** 2.2
    return xyz_to_srgb(WP[0] * a, WP[1] * a, WP[2] * a)


def calrgb_truth(x, y):
    """ISO 32000-1 8.6.5.3: XYZ = Matrix . (A^Ga, B^Gb, C^Gc).

    `/Matrix` is column-major in the PDF array -- `[Xa Ya Za Xb Yb Zb Xc Yc
    Zc]` -- so column A is the first THREE numbers, not every third one. The
    fixture uses the sRGB primaries, which are asymmetric enough that a
    transposed read produces visibly different numbers here.
    """
    a = (x * 4 / 255.0) ** 2.2
    b = (y * 4 / 255.0) ** 2.2
    c = (128 / 255.0) ** 2.2
    xa, ya, za = 0.4124, 0.2126, 0.0193
    xb, yb, zb = 0.3576, 0.7152, 0.1192
    xc, yc, zc = 0.1805, 0.0722, 0.9505
    return xyz_to_srgb(xa * a + xb * b + xc * c,
                       ya * a + yb * b + yc * c,
                       za * a + zb * b + zc * c)


ORACLES = {"lab": lab_truth, "calgray": calgray_truth, "calrgb": calrgb_truth}


def cli_path() -> str:
    """Absolute path to the release CLI, resolved from the repo root.

    Resolved from THIS FILE's location rather than the cwd: the script is
    useful from any directory (the fixtures live outside the repo), and a
    relative `target/release/...` silently becomes a `FileNotFoundError`
    from `CreateProcess` the moment it is run from anywhere else.
    """
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    for cand in ("pdfcer.exe", "pdfcer"):
        p = os.path.join(root, "target", "release", cand)
        if os.path.exists(p):
            return p
    sys.exit("build the release CLI first: cargo build --release -p pdfcer-cli")


def render_pdfce(pdf: str, png: str) -> None:
    subprocess.run(
        [cli_path(), "render-page", pdf, "--page", "1",
         "--scale", f"{SCALE:.6f}", "-o", png],
        check=True, capture_output=True,
    )


def measure(pdf: str, oracle, tmp: str):
    png = os.path.join(tmp, os.path.basename(pdf) + ".pdfcer.png")
    render_pdfce(pdf, png)
    ours = np.asarray(Image.open(png).convert("RGB")).astype(np.int16)
    h, w = ours.shape[:2]
    page = pdfium.PdfDocument(pdf)[0]
    ref = np.asarray(
        page.render(scale=w / page.get_width()).to_pil().convert("RGB")
    ).astype(np.int16)[:h, :w]

    e_ours, e_ref = [], []
    worst = None
    for y in range(BORDER, H - BORDER):
        for x in range(BORDER, W - BORDER):
            t = oracle(x, y)
            px = int((x + 0.5) / W * w)
            py = int((y + 0.5) / H * h)
            o = tuple(int(v) for v in ours[py, px])
            r = tuple(int(v) for v in ref[py, px])
            eo = max(abs(t[i] - o[i]) for i in range(3))
            er = max(abs(t[i] - r[i]) for i in range(3))
            e_ours.append(eo)
            e_ref.append(er)
            if worst is None or eo > worst[0]:
                worst = (eo, x, y, t, o, r)
    return np.array(e_ours), np.array(e_ref), worst


def main() -> int:
    if len(sys.argv) < 2:
        sys.exit("usage: check-image-colorspace-truth.py <fixture-dir> [--json]")
    fixdir = sys.argv[1]
    as_json = "--json" in sys.argv
    tmp = os.path.join(fixdir, "_truth")
    os.makedirs(tmp, exist_ok=True)

    report, failed = {}, False
    tol = 2
    for name, oracle in ORACLES.items():
        pdf = os.path.join(fixdir, name + ".pdf")
        if not os.path.exists(pdf):
            print(f"{name:9s} MISSING  (run tools/gen-image-colorspace-fixtures.py)")
            failed = True
            continue
        eo, er, worst = measure(pdf, oracle, tmp)
        ok = eo.max() <= tol
        failed |= not ok
        report[name] = {
            "texels": int(eo.size),
            "pdfcer_mean": float(eo.mean()), "pdfcer_max": int(eo.max()),
            "pdfium_mean": float(er.mean()), "pdfium_max": int(er.max()),
            "ok": bool(ok),
        }
        if not as_json:
            print(f"{name:9s} n={eo.size:5d}  "
                  f"pdfcer mean={eo.mean():6.3f} max={eo.max():4d}  "
                  f"{'OK ' if ok else 'FAIL'}   "
                  f"| pdfium mean={er.mean():7.3f} max={er.max():4d}")
            if worst and worst[0] > 0:
                _, x, y, t, o, r = worst
                print(f"          worst texel ({x},{y}) truth={t} pdfcer={o} pdfium={r}")
    if as_json:
        print(json.dumps(report, indent=2))
    else:
        print(f"\ntolerance: pdfcer max error must be <= {tol}/255. "
              "pdfium's column is REPORTED, never gated.")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
