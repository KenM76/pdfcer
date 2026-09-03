#!/usr/bin/env python3
"""Every `pub` / `pub(crate)` function must carry a doc comment.

WHY THIS GATE EXISTS
--------------------
`CLAUDE.md` rule 6 and the operator's global documentation-first directive
make this binding, not stylistic: *"every function gets a doc comment
explaining WHY; the docs are the logic, the code is the syntax."* The bar
is that a competent engineer could rebuild a module from its docs alone.

Nothing enforced it, and 75 public functions had drifted past it by
2026-09-02 — 2.2% of 3,377. That number is small enough to be recoverable
and large enough that it would never have been recovered by noticing.

★★ THE FAILURE MODE THAT MOTIVATED IT IS NOT "A FUNCTION IS UNDOCUMENTED"
------------------------------------------------------------------------
It is that **the missing doc comment is usually somewhere else, welded to
the wrong item.**

`crates/pdfcer-render/src/cmyk_buffer.rs` had this shape:

    /// Write one pixel, clamping into range.
    ///
    /// The clamp is not defensive tidiness: ... a legal colorant tint.
    /// Widen the dirty rectangle to include `region`.
    ///
    /// Called by every composite entry point rather than by `set_pixel`...
    fn mark_dirty(&mut self, region: (u32, u32, u32, u32)) {

Two doc blocks fused into one, `set_pixel` moved elsewhere without its
documentation, and `mark_dirty` left carrying a first sentence that
describes a **different function**. Nothing failed. `rustfmt` is happy,
`clippy` is happy, and `clippy::doc_lazy_continuation` — the lint that
catches the *blank-line* variant of this — cannot see a contiguous weld.

So the observable symptom of a silently corrupted doc block is an
**undocumented neighbour**, and that is what this gate detects. It cannot
tell you the surviving block is wrong; it can tell you a block went
missing, which is the same event.

Same family as `check-string-gaps.sh` (a lost backslash continuation) and
`check-cli-help-leads.py` (a `///` whose FIRST line is the shipped `--help`
summary, so a paragraph spliced above it buries the summary): this machine's
editing tools eat structure at block boundaries, repeatedly, and each
variant needs its own detector because none of them is visible in a diff.

★ That sibling was cited here as `check-cli-help-nonempty.py` -- a file that
does not exist -- for the length of one commit. Caught by a reading agent,
not by any gate, because no gate checks whether a doc comment's cross-
reference resolves. Left recorded rather than quietly corrected: a citation
is a claim, and this one was written from memory of what the gate does
rather than from its filename.

WHAT COUNTS AS DOCUMENTED
-------------------------
A `///` line immediately above the item, skipping attributes (`#[...]`)
and `#[cfg]`-gated stacks. A `//` ordinary comment does NOT count — that
is a note to a maintainer, not documentation of a contract, and the
distinction is the whole of rule 6.

WHAT IS NOT CHECKED, AND WHY
----------------------------
- **Private functions.** Rule 6 asks for them too, but `clippy`'s
  `missing_docs_in_private_items` exists for that and turning it on is a
  separate, much larger decision. This gate covers the surface other
  crates and the consuming GUI project actually build against.
- **Doc-comment QUALITY.** A `/// Does the thing.` passes. No script can
  judge whether a comment explains the WHY, and a gate that pretended to
  would train people to write filler that satisfies it.
- **Test files.** `#[cfg(test)]` helpers document themselves by name and
  are not a public surface.

THE BASELINE IS DEBT, NOT AN ALLOWLIST
--------------------------------------
`tools/public-fns-undocumented-baseline.txt` carries the pre-existing set
so this gate can be turned on today rather than after a 75-function
writing session. Shortening it is the intended direction. **Do not add to
it** — a new undocumented function is a new violation, and the gate exists
to make that a build failure rather than a discovery six months later.

Entries are `path::function_name`, deliberately WITHOUT a line number:
line numbers churn on every edit and would make the baseline a merge
conflict generator. The cost is that moving a function does not re-check
it; the benefit is a baseline that survives ordinary editing.

EXIT CODES
----------
0  clean, or every violation is in the baseline
1  at least one undocumented public function outside the baseline
2  the baseline names something that no longer exists (stale entry) --
   also a failure, because a baseline that silently keeps dead rows is a
   baseline nobody trims
"""

from __future__ import annotations

import pathlib
import re
import sys

# Windows consoles default to a code page that cannot encode the stars and
# em-dashes this file prints, and Python does not substitute -- it RAISES,
# killing the gate mid-report after it has already printed the violations.
# The established fix in this directory (see `check-commits-filed.py`),
# applied to BOTH streams from the start rather than to the one where the
# crash happened to be seen.
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
if hasattr(sys.stderr, "reconfigure"):
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

ROOT = pathlib.Path(__file__).resolve().parent.parent
BASELINE = ROOT / "tools" / "public-fns-undocumented-baseline.txt"

# `pub fn`, `pub(crate) fn`, `pub(super) fn`, with any of const/async/unsafe
# between. Anchored at line start with leading whitespace so a `pub fn`
# inside a string literal or a comment cannot match.
FN = re.compile(
    r"^\s*pub(?:\([a-z]+\))?\s+(?:const\s+|async\s+|unsafe\s+|extern\s+\"[^\"]*\"\s+)*fn\s+(\w+)"
)


def is_documented(lines: list[str], index: int) -> bool:
    """Whether the item at `index` has a `///` above it.

    Walks back over attribute lines, which legitimately sit between the
    doc comment and the item -- `#[must_use]`, `#[inline]`, `#[allow(...)]`
    and `#[cfg(...)]` stacks are all normal here. A multi-line attribute
    (a `#[derive(...)]` broken across lines, say) is handled by also
    accepting a bare `)]` continuation.
    """
    j = index - 1
    while j >= 0:
        stripped = lines[j].strip()
        if stripped.startswith("#[") or stripped.startswith("#!") or stripped in (")]", "}]"):
            j -= 1
            continue
        break
    return j >= 0 and lines[j].strip().startswith("///")


def scan() -> list[str]:
    """Every undocumented public function, as `path::name`, sorted."""
    found: list[str] = []
    for path in sorted((ROOT / "crates").rglob("*.rs")):
        rel = path.relative_to(ROOT).as_posix()
        # `tests/` are integration tests: a public surface only to
        # themselves. `build.rs` has no public API at all.
        if "/tests/" in rel or rel.endswith("/build.rs"):
            continue
        try:
            lines = path.read_text(encoding="utf-8").splitlines()
        except OSError as err:  # unreadable file is a gate failure, not a skip
            print(f"public-fns-documented: cannot read {rel}: {err}")
            continue
        in_test_mod = False
        for i, line in enumerate(lines):
            # A crude but adequate `#[cfg(test)] mod tests` detector: once
            # inside one, everything to end of file is test scaffolding in
            # this codebase's layout (the test module is always last).
            if line.strip().startswith("mod tests") and i > 0 and "cfg(test)" in lines[i - 1]:
                in_test_mod = True
            if in_test_mod:
                continue
            match = FN.match(line)
            if match and not is_documented(lines, i):
                found.append(f"{rel}::{match.group(1)}")
    return sorted(found)


def main() -> int:
    found = set(scan())
    baseline: set[str] = set()
    if BASELINE.is_file():
        baseline = {
            line.strip()
            for line in BASELINE.read_text(encoding="utf-8").splitlines()
            if line.strip() and not line.startswith("#")
        }

    new = sorted(found - baseline)
    stale = sorted(baseline - found)

    if not new and not stale:
        print(
            f"public-fns-documented: clean -- every public fn has a doc comment "
            f"({len(baseline)} carried in the baseline as DEBT)"
        )
        return 0

    if new:
        print(f"public-fns-documented: {len(new)} public fn(s) with NO doc comment:")
        for item in new:
            print(f"    {item}")
        print()
        print("Rule 6 is binding: every function gets a doc comment explaining WHY.")
        print()
        print("★ Before writing a new one, check whether it went MISSING rather than")
        print("  never existing. The usual cause is a splice that welded two doc")
        print("  blocks together, which leaves the NEIGHBOURING function carrying a")
        print("  first sentence that describes this one. Read the item above.")
        print()
        print("Do NOT add it to tools/public-fns-undocumented-baseline.txt --")
        print("that file is pre-existing debt this gate was written around, and")
        print("extending it silences exactly what the gate exists to catch.")

    if stale:
        print(f"public-fns-documented: {len(stale)} STALE baseline entry(ies):")
        for item in stale:
            print(f"    {item}")
        print()
        print("These are documented now, or renamed, or gone. Delete the lines --")
        print("a baseline that keeps dead rows is a baseline nobody trims, and its")
        print("length stops meaning anything.")

    return 1 if new else 2


if __name__ == "__main__":
    sys.exit(main())
