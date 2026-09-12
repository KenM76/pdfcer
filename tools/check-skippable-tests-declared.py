#!/usr/bin/env python3
"""Every test that can skip itself must be DECLARED, with what it needs.

WHY THIS GATE EXISTS
====================

A test that returns early because a fixture is missing prints `SKIP` and
**passes**. `cargo test` reports it in the passed count. Nothing is red,
nothing is amber, and a reader of the summary has no way to tell a test that
ran from a test that declined to.

★★ MEASURED 2026-09-12, and the number is the argument:

    editable_roundtrip            2 of  6 "passed" are skips
    insert_pages_preserves_undo   1 of  7
    merge_document                8 of 16
    structure_inspect             1 of 12
    widget_adoption              14 of 20
                                 --------
                                 26 tests

★★★ AND THEY SKIP IN CI TOO. `fixtures/external/` is **not tracked in git**
(`git ls-files fixtures/external` → 0) and no workflow step fetches it. So
these 26 tests have **never executed in continuous integration**. They are not
"covered on the runner and skipped locally"; they are green everywhere and run
nowhere.

It was found the hard way: a guard shipped for `Pass 298.0` was sabotaged to
prove its test could fail, and the test **stayed green** — because it was one
of the fourteen in `widget_adoption.rs`. The consuming shell had reported this
exact shape about its own harness four days earlier ("a SKIP is not red;
nothing went amber for weeks"), and it was reproduced here within the hour.

WHAT THIS GATE DOES, AND WHAT IT DELIBERATELY DOES NOT
======================================================

It does **not** run the tests — that would cost minutes and answer a question
the source already answers. It scans `crates/*/tests/*.rs` for the
skip idiom and requires every occurrence to be listed in
`tools/skippable-tests-baseline.txt`, one `path::marker` per line.

* A **new** skippable test is a finding: either give it a fixture it cannot
  miss, or declare it deliberately.
* A **stale** entry is also a finding, for the reason every baseline in this
  directory gives: a baseline that keeps dead rows is a baseline nobody trims.

★ The baseline is DEBT and the direction is down. The fix for a row is not to
keep it tidy — it is to build the fixture synthetically in the test file, as
`widget_adoption.rs`'s `synthetic_orphan_widget()` now does, so the test cannot
decline to run.

WHAT IT CANNOT SEE
==================

A test that returns early *without* printing anything. The idiom this codebase
uses is `eprintln!("SKIP: …"); return;`, and that is what is matched. A silent
early return is a worse version of the same defect and would need the test to
be run to detect — if one is ever found, that is the measurement that justifies
widening this.

USAGE
=====

    python tools/check-skippable-tests-declared.py [--stats] [--write]

`--write` regenerates the baseline; use it only when deliberately accepting a
new skippable test. Exit 0 clean, 1 on a finding.
"""

from __future__ import annotations

import io
import pathlib
import re
import sys

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")

ROOT = pathlib.Path(__file__).resolve().parent.parent
BASELINE = ROOT / "tools" / "skippable-tests-baseline.txt"

SKIP = re.compile(r'eprintln!\s*\(\s*"SKIP')
# `fn some_test_name(` at the start of a line, which in a tests/ file is a
# test or a helper; the marker is the function the skip lives in.
FN = re.compile(r"^fn (\w+)\s*\(")


def scan() -> list[str]:
    """Every `path::function` containing the skip idiom, sorted."""
    found: list[str] = []
    for path in sorted((ROOT / "crates").glob("*/tests/*.rs")):
        rel = path.relative_to(ROOT).as_posix()
        current = "<file scope>"
        for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
            m = FN.match(line)
            if m:
                current = m.group(1)
            if SKIP.search(line):
                marker = f"{rel}::{current}"
                if marker not in found:
                    found.append(marker)
    return sorted(found)


def main() -> int:
    found = scan()

    if "--write" in sys.argv:
        BASELINE.write_text(
            "# Tests that can decline to run, and therefore pass without\n"
            "# running. DEBT -- the direction is down. See\n"
            "# tools/check-skippable-tests-declared.py for why.\n"
            + "\n".join(found)
            + "\n",
            encoding="utf-8",
        )
        print(f"check-skippable-tests-declared: baseline written, {len(found)} entry(ies)")
        return 0

    baseline: list[str] = []
    if BASELINE.is_file():
        baseline = [
            l.strip()
            for l in BASELINE.read_text(encoding="utf-8", errors="replace").splitlines()
            if l.strip() and not l.startswith("#")
        ]

    new = sorted(set(found) - set(baseline))
    stale = sorted(set(baseline) - set(found))

    if "--stats" in sys.argv:
        print(f"  skippable sites : {len(found)}")
        print(f"  declared        : {len(baseline)}")

    if new:
        print(f"check-skippable-tests-declared: {len(new)} UNDECLARED skippable test(s):")
        for m in new:
            print(f"    {m}")
        print()
        print(
            "  A test that returns early on a missing fixture PASSES. It is\n"
            "  counted as passed by `cargo test` and nothing goes amber.\n"
            "\n"
            "  Prefer building the fixture synthetically in the test file so the\n"
            "  test cannot decline to run -- see `synthetic_orphan_widget()` in\n"
            "  crates/pdfcer-core/tests/widget_adoption.rs. Declare it with\n"
            "  --write only if you mean it."
        )
        return 1

    if stale:
        print(f"check-skippable-tests-declared: {len(stale)} STALE entry(ies):")
        for m in stale:
            print(f"    {m}")
        print()
        print("  These no longer skip -- good. Re-run with --write to trim them.")
        return 1

    print(
        f"check-skippable-tests-declared: clean — {len(found)} declared skippable "
        f"test(s), carried as DEBT"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
