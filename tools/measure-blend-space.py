"""measure-blend-space.py — how much of a corpus blends in the wrong space.

WHY THIS EXISTS
===============

ISO 32000-1 §11.3.4 requires a SUBTRACTIVE blending colour space
(`DeviceCMYK`, `Separation`, `DeviceN`) to have its components
**complemented before the blend function and complemented back after it**.
Until `Pass 97.1e` (2026-08-21) pdfcer blended in device sRGB, so every
non-`Normal` blend inside such a group was computed on the wrong side of
that switch — and the marks landed in the right places, so the page looked
plausible. A counter was the only way to see it, and this script is how
that counter is read at corpus scale.

★ THE COUNTER'S MEANING CHANGED WITH THE FIX, AND THIS DOCSTRING WAS THE
LAST PLACE STILL DESCRIBING THE OLD WORLD. A page whose group declares a
subtractive space now composites in a colorant buffer, and
`blends_in_wrong_space` increments only where that did NOT happen. On the
suite this script reported 107 of 107 wrong before the fix and 0 of
107 after it — which is the whole reason the counter had to be narrowed:
left as it was, it would have gone on reporting 107 while two patches
started passing, and this script is the only instrument anybody runs at
corpus scale for the question.

WHAT THE TWO NUMBERS MEAN, AND WHY BOTH
=======================================

`blend_space_subtractive` is a CENSUS — the page and every transparency
group whose blending space is subtractive. It is EXPOSURE, not error: the
complement applies to the blend FUNCTION, and `Normal` is `c_s` on either
side of it, so a page can be entirely `DeviceCMYK` and entirely correct.
Reporting this alone would call every CMYK print file in the world broken.

`blends_in_wrong_space` is the SHORTFALL — non-`Normal` modes actually
computed additively inside one. This is the number that says a rendering is
affected.

THE FIRST MEASUREMENT, 2026-08-21, so a later run has something to differ
from
=======================================================================

    print-conformance suite (51)   13 files subtractive   107/107 wrong (100.0%)
    fixtures/external (3,735)     15 files subtractive     2/49  wrong (  4.1%)

One hundred percent on the suite built to test this; four percent on the
corpus of files people actually have. Both matter and they say different
things: the suite transparency panels cannot pass without the colorant
buffer, and the buffer will change almost nothing about how ordinary
documents look. That second half is a SCOPING fact and the suite numbers
alone could not have produced it.

USAGE
=====

::

    python tools/measure-blend-space.py <corpus-dir> [<corpus-dir> ...]

Renders page 1 of every PDF at scale 1 through the release `pdfcer` and
parses its stable stdout line. Nothing is written into the repository; the
corpus lives outside it (`docs/LEGAL.md` §5).
"""
import glob
import os
import re
import subprocess
import sys
import tempfile

CLI = os.path.abspath(r"D:\Dev\pdfcer\target\release\pdfcer.exe")
assert os.path.isfile(CLI), CLI

G = re.compile(r"blend_space_subtractive=(\d+)")
B = re.compile(r"blends_in_wrong_space=(\d+)")
M = re.compile(r"blend_modes_applied=(\d+)")

out_png = os.path.join(tempfile.gettempdir(), "measure_space.png")


def run(root, limit=0):
    files = sorted(glob.glob(os.path.join(root, "**", "*.pdf"), recursive=True))
    if limit:
        files = files[:limit]
    n = files_sub = files_wrong = tot_g = tot_b = tot_m = 0
    worst = []
    for f in files:
        try:
            p = subprocess.run(
                [CLI, "render-page", f, "--page", "1", "--scale", "1", "-o", out_png],
                capture_output=True, text=True, errors="replace", timeout=60,
            )
        except subprocess.TimeoutExpired:
            continue
        s = p.stdout
        if "blend_space_subtractive=" not in s:
            continue
        n += 1
        g = int(G.search(s).group(1))
        b = int(B.search(s).group(1))
        m = int(M.search(s).group(1))
        tot_g += g
        tot_b += b
        tot_m += m
        if g:
            files_sub += 1
        if b:
            files_wrong += 1
            worst.append((b, g, os.path.relpath(f, root)))
    print(f"\n{root}")
    print(f"  files rendered ............ {n}")
    print(f"  with a SUBTRACTIVE space .. {files_sub}  ({100*files_sub/max(n,1):.1f}%)")
    print(f"  with a WRONG-SPACE blend .. {files_wrong}  ({100*files_wrong/max(n,1):.1f}%)")
    print(f"  subtractive groups (total)  {tot_g}")
    print(f"  blend modes applied ....... {tot_m}")
    print(f"  of those, in the wrong space {tot_b}"
          + (f"  ({100*tot_b/tot_m:.1f}%)" if tot_m else ""))
    for b, g, name in sorted(worst, reverse=True)[:10]:
        print(f"    {b:5d} wrong / {g:5d} groups   {name[:70]}")


if __name__ == "__main__":
    for root in sys.argv[1:]:
        run(root)
