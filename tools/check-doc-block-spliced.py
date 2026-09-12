#!/usr/bin/env python3
"""One doc block must not contain the same heading twice.

WHY THIS GATE EXISTS
====================

An edit that lands **between a doc block and the item it documents** welds two
blocks into one. The item below ends up carrying a first sentence that
describes a *different* function, and the function whose block was absorbed
ends up bare.

`check-public-fns-documented.py` detects the second half of that — the bare
neighbour — and its own header explains the mechanism at length. It has two
blind spots, and this gate exists for both:

1. **It only looks at `pub` items.** A splice that lands on a private function
   is invisible to it. (Widening its denominator was measured and rejected:
   1,837 private functions outside test modules have no doc comment, which
   would be a baseline nobody reads.)

2. ★★ **A baseline entry can be HIDING a splice rather than recording an
   omission**, and this is the finding that produced this file.
   `EditSession::delete_subpath` sits in
   `tools/public-fns-undocumented-baseline.txt` as pre-existing debt. It is
   not undocumented: **its doc block is thirty lines up, welded onto
   `delete_node`**, whose rustdoc therefore opens *"Delete one subpath of the
   path object…"*. The text was never missing. It was misfiled, and a baseline
   row said "nobody wrote this" for as long as anybody cared to read it.

THE SIGNAL, AND WHY IT IS EXACT RATHER THAN HEURISTIC
=====================================================

Rustdoc convention gives a block **at most one** `# Errors`, `# Returns`,
`# Examples`, `# Panics` or `# Safety` section. Two of the same heading inside
one contiguous `///` run is not a style choice anybody makes — it is two
documents in one block.

Matching on headings rather than on prose is what makes this cheap and quiet:
no natural-language guessing, no similarity scoring, and a finding a reader can
confirm in one glance at the two line numbers.

WHAT IT SKIPS, AND WHY EACH ONE MATTERS
=======================================

* **Fenced code.** Inside a ```` ``` ```` block, `/// # use std::fmt;` is a
  rustdoc *hidden doc-test line*, not a heading. Without this, every doc
  example that hides two setup lines is a false positive — it was the first
  thing this gate got wrong, on `EditSession::delete_annotation`.

* **Headings are normalised** before comparison (case folded, decoration
  stripped), so `# ★★ Errors` and `# Errors` are one heading. A splice does not
  become invisible because one half of it was starred.

USAGE
=====

    python tools/check-doc-block-spliced.py [--stats]

Exit 0 clean, 1 on a finding. There is deliberately **no baseline**: the tree
was clean the day this shipped, having had its four live splices repaired, and
a gate with nothing to carry should not grow the machinery for carrying it.
"""

from __future__ import annotations

import collections
import pathlib
import re
import sys

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")

ROOT = pathlib.Path(__file__).resolve().parent.parent

DOC = re.compile(r"^\s*///\s?(.*)$")
HEADING = re.compile(r"^#+\s+(.+?)\s*$")
FENCE = re.compile(r"^\s*```")


def normalise(heading: str) -> str:
    """`# ★★ Errors` and `# Errors` are the same heading."""
    return re.sub(r"[^a-z0-9 ]", "", heading.lower()).strip()


def blocks(lines: list[str]):
    """Each contiguous `///` run, as `(start_line_number, [content])`."""
    run: list[str] = []
    start = 0
    for i, line in enumerate(lines):
        m = DOC.match(line)
        if m:
            if not run:
                start = i + 1
            run.append(m.group(1))
        elif run:
            yield start, run
            run = []
    if run:
        yield start, run


def repeated_headings(content: list[str]) -> dict[str, int]:
    """Normalised headings appearing more than once, outside fenced code."""
    seen: collections.Counter[str] = collections.Counter()
    in_fence = False
    for line in content:
        if FENCE.match(line):
            in_fence = not in_fence
            continue
        if in_fence:
            # `# use std::fmt;` here is a HIDDEN DOC-TEST LINE, not a heading.
            continue
        h = HEADING.match(line)
        if h:
            key = normalise(h.group(1))
            if key:
                seen[key] += 1
    return {k: v for k, v in seen.items() if v > 1}


def main() -> int:
    findings: list[tuple[str, int, str, int]] = []
    checked = 0
    for path in sorted((ROOT / "crates").rglob("*.rs")):
        rel = path.relative_to(ROOT).as_posix()
        if "/tests/" in rel or rel.endswith("/build.rs"):
            continue
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
        for start, content in blocks(lines):
            checked += 1
            for heading, count in sorted(repeated_headings(content).items()):
                findings.append((rel, start, heading, count))

    if "--stats" in sys.argv:
        print(f"  doc blocks scanned : {checked}")
        print(f"  findings           : {len(findings)}")

    if findings:
        print("check-doc-block-spliced: FINDINGS —")
        for rel, start, heading, count in findings:
            print(f"  {rel}:{start}: heading '{heading}' appears {count}x in ONE doc block")
        print()
        print(
            "  Two of the same heading in one block is two documents welded\n"
            "  together by an edit that landed between a block and its item. The\n"
            "  item below now opens by describing a DIFFERENT function, and the\n"
            "  function whose block was absorbed is bare.\n"
            "\n"
            "  Split the block and reattach the absorbed half. Check whether the\n"
            "  bare function is sitting in tools/public-fns-undocumented-baseline.txt\n"
            "  -- a row there can be hiding a splice rather than an omission, which\n"
            "  is why this gate exists."
        )
        return 1

    print(f"check-doc-block-spliced: clean — {checked} doc block(s), no repeated heading")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
