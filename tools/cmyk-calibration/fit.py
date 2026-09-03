#!/usr/bin/env python3
"""fit.py — derive and score the calibrated DeviceCMYK -> sRGB node grid.

WHY THIS EXISTS
---------------
`cmyk_probe.py` produces ground truth: measured sRGB output for a set of known
DeviceCMYK inputs, read back from a rendered page. This script is the analysis
half. It

  1. scores candidate conversions against that ground truth,
  2. fits the node grid `pdfcer-core/src/color.rs` ships, and
  3. emits that grid as Rust source.

It is a DEVELOPMENT tool. Nothing it produces is loaded at pdfcer runtime — the
fitted nodes are pasted into `color.rs` as a `const`, so the shipped binary
carries a table and no data file, no parser, and no I/O. Re-running this is how
a future engineer re-derives, checks, or retargets those numbers instead of
trusting them.

WHAT IS BEING FITTED (full rationale in `pdfcer-core/src/color.rs`)
-----------------------------------------------------------------
Quadrilinear interpolation over a uniform `L x L x L x L` grid of nodes in the
CMYK unit hypercube. Each node holds an sRGB triple. Given (c,m,y,k) the
conversion locates the enclosing cell and blends its 16 corner nodes by the
product of the per-axis fractions.

Fitting the nodes is a LINEAR least-squares problem — the interpolation weights
are the design matrix and the node colours are the unknowns — so there is a
closed-form optimum, no iterative solver, no random seed, and no knob to turn.
That matters for the project's W14 rule (never tune a threshold until a number
turns green): there is no threshold here, only a least-squares solution that
either fits the measurements or doesn't, and a reported error either way.

`L` is the one structural choice. It trades table size (L^4 nodes) against
accuracy; per-pixel cost is INDEPENDENT of L, because a quadrilinear lookup
always touches exactly 16 nodes no matter how many the table has. §"choosing L"
in the README records the measured sweep behind the shipped value.

VALIDATION DISCIPLINE
---------------------
Score on a set the fit never saw, and prefer `--random` probe output over a
lattice: a lattice validation set silently coincides with the model's own grid
nodes whenever the two resolutions share a divisor, which degenerates
validation into "can it reproduce points it was handed" and flatters the fit.

USAGE
-----
    python fit.py --fit out/fit-pdfium.tsv --validate out/val-random.tsv
    python fit.py --fit out/fit-pdfium.tsv --sweep
    python fit.py --fit out/fit-pdfium.tsv --levels 6 --emit-rust
"""

from __future__ import annotations

import argparse
import itertools
from pathlib import Path

import numpy as np

# The shipped grid resolution. See README "choosing L".
DEFAULT_L = 6


def load(path: Path):
    """Read a probe TSV into (cmyk[N,4] in 0..1, rgb[N,3] in 0..255)."""
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line or line.startswith("#") or line.startswith("c\t"):
            continue
        rows.append([float(v) for v in line.split("\t")])
    a = np.asarray(rows, dtype=np.float64)
    return a[:, :4], a[:, 4:7]


# --- models -----------------------------------------------------------------


def naive_additive(cmyk):
    """pdfcer's pre-change conversion: `1 - min(1, x + k)` per channel."""
    return np.clip(1.0 - np.minimum(1.0, cmyk[:, :3] + cmyk[:, 3:4]), 0.0, 1.0)


def multiplicative(cmyk):
    """The other data-free closed form: `(1 - x) * (1 - k)`."""
    return (1.0 - cmyk[:, :3]) * (1.0 - cmyk[:, 3:4])


def weights(cmyk, L: int):
    """Quadrilinear weight matrix [N, L^4] for an L-level-per-axis grid.

    Node index is row-major in (c, m, y, k) — c slowest, k fastest — which is
    the order `color.rs` uses, so the emitted table can be pasted without a
    reshuffle. Each row sums to 1.
    """
    n = cmyk.shape[0]
    base, frac = [], []
    for axis in range(4):
        t = np.clip(cmyk[:, axis], 0.0, 1.0) * (L - 1)
        i0 = np.minimum(np.floor(t).astype(int), L - 2)
        base.append(i0)
        frac.append(t - i0)
    w = np.zeros((n, L**4))
    rows = np.arange(n)
    for bits in itertools.product((0, 1), repeat=4):
        contrib = np.ones(n)
        node = np.zeros(n, dtype=int)
        for axis, bit in enumerate(bits):
            contrib *= frac[axis] if bit else (1.0 - frac[axis])
            node = node * L + (base[axis] + bit)
        np.add.at(w, (rows, node), contrib)
    return w


def fit_nodes(cmyk, rgb01, L: int):
    """Least-squares node colours, with the 16 hypercube corners snapped to
    their directly measured values.

    WHY SNAP. The 16 corners are the named ink combinations — solid cyan,
    C+M blue, solid black ink, paper white, four-colour solid. They are what a
    person looks at when they check whether the conversion is right, and they
    are the only lattice points a grid node coincides with exactly (0.0 and 1.0
    are members of every lattice, so the fit set measures them directly). The
    unsnapped least-squares answer misses solid cyan by ~3/255 because it is
    also trying to fit the cell interior around it — a trade that is invisible
    in aggregate and conspicuous at the one value someone will name.

    White is additionally forced to exactly 1.0 and four-colour solid to
    exactly 0.0. `0 0 0 0 k` is how a producer paints opaque paper white over a
    white page, and a 254 there is a visible pale rectangle; the darkest value
    the space can express must not float above zero.

    MEASURED COST OF SNAPPING: none. On a 4,000-point random validation set the
    snapped and unsnapped grids score identically (mean 1.16, p95 5.8, 2.57 %
    over 8/255) — the correction is confined to the corner cells and is smaller
    there than the interpolation error it displaces.
    """
    nodes, *_ = np.linalg.lstsq(weights(cmyk, L), rgb01, rcond=None)
    nodes = np.clip(nodes, 0.0, 1.0)

    for corner in itertools.product((0, L - 1), repeat=4):
        node = 0
        for axis_index in corner:
            node = node * L + axis_index
        # The measurement at this corner: the fit set's row whose CMYK is the
        # corresponding 0/1 combination.
        want = np.array([0.0 if i == 0 else 1.0 for i in corner])
        row = int(np.argmin(np.abs(cmyk - want).sum(axis=1)))
        assert np.allclose(cmyk[row], want), "fit set does not sample the hypercube corners"
        nodes[node] = rgb01[row]

    nodes[0] = 1.0
    nodes[-1] = 0.0
    return nodes


def apply_nodes(cmyk, nodes, L: int):
    return np.clip(weights(cmyk, L) @ nodes, 0.0, 1.0)


# --- scoring ----------------------------------------------------------------


def score(name, pred01, rgb01):
    """The same shape of numbers decision 006 §3.7 reported, so the two compare.

    `frac_gt8` in particular is 006's headline: the fraction of samples where
    SOME channel differs by more than 8/255 from the reference.
    """
    pred = np.clip(pred01, 0.0, 1.0) * 255.0
    ref = rgb01 * 255.0
    err = np.abs(pred - ref)
    chan_max = err.max(axis=1)
    return {
        "name": name,
        "mean": err.mean(),
        "p95": np.percentile(chan_max, 95),
        "max": err.max(),
        "frac_gt8": (chan_max > 8).mean() * 100.0,
        "frac_gt16": (chan_max > 16).mean() * 100.0,
        "frac_gt32": (chan_max > 32).mean() * 100.0,
    }


def print_scores(rows):
    print(f"{'model':<28}{'mean':>7}{'p95':>7}{'max':>6}{'>8/255':>9}{'>16':>8}{'>32':>8}")
    for r in rows:
        print(
            f"{r['name']:<28}{r['mean']:>7.2f}{r['p95']:>7.1f}{r['max']:>6.0f}"
            f"{r['frac_gt8']:>8.1f}%{r['frac_gt16']:>7.1f}%{r['frac_gt32']:>7.1f}%"
        )


def emit_rust(nodes, L: int, src: str):
    print(f"// GRID_L = {L}; {L**4} nodes; fitted from {src}")
    print(f"const GRID_L: usize = {L};")
    print(f"const NODES: [[f32; 3]; {L**4}] = [")
    for i, (r, g, b) in enumerate(nodes):
        ci = i // (L**3)
        mi = (i // (L**2)) % L
        yi = (i // L) % L
        ki = i % L
        print(f"    [{r:.6f}, {g:.6f}, {b:.6f}], // c{ci} m{mi} y{yi} k{ki}")
    print("];")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--fit", type=Path, required=True)
    ap.add_argument("--validate", type=Path)
    ap.add_argument("--levels", type=int, default=DEFAULT_L)
    ap.add_argument("--sweep", action="store_true", help="score L = 2..7 instead of one L")
    ap.add_argument("--emit-rust", action="store_true")
    args = ap.parse_args()

    cmyk, rgb = load(args.fit)
    rgb01 = rgb / 255.0
    print(f"== fit set: {args.fit} ({len(cmyk)} samples)")

    levels = range(2, 8) if args.sweep else [args.levels]
    fitted = {L: fit_nodes(cmyk, rgb01, L) for L in levels}

    rows = [
        score("naive-additive (before)", naive_additive(cmyk), rgb01),
        score("multiplicative", multiplicative(cmyk), rgb01),
    ]
    rows += [
        score(f"quadrilinear L={L} ({L**4}n)", apply_nodes(cmyk, fitted[L], L), rgb01) for L in levels
    ]
    print_scores(rows)

    if args.validate:
        vc, vr = load(args.validate)
        vr01 = vr / 255.0
        vrows = [
            score("naive-additive (before)", naive_additive(vc), vr01),
            score("multiplicative", multiplicative(vc), vr01),
        ]
        vrows += [
            score(f"quadrilinear L={L} ({L**4}n)", apply_nodes(vc, fitted[L], L), vr01) for L in levels
        ]
        print(f"\n== validation set: {args.validate} ({len(vc)} samples, out-of-sample)")
        print_scores(vrows)

    if args.emit_rust:
        print()
        emit_rust(fitted[args.levels], args.levels, args.fit.name)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
