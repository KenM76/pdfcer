#!/usr/bin/env python3
"""Format-check the crates `cargo fmt --all` structurally cannot see.

WHY THIS GATE EXISTS
====================

`cargo fmt --all --check` — the command CI runs, and the one project rule 10
names — formats every *workspace member*. pdfcer deliberately keeps several
crates OUT of the workspace (`Cargo.toml`'s `exclude` list): the differential
test oracle, the corpus sweep tools, and the `cargo-fuzz` harness. Those
exclusions are correct and load-bearing, for the reasons the root manifest
gives: they keep `cargo test` fast, keep the GUI-core-separation `cargo tree`
checks unambiguous, and keep `cargo-about`'s generated
`THIRD_PARTY_LICENSES.md` a picture of the SHIPPING dependency graph only.

But the exclusion has a side effect nobody chose: **excluded crates are
invisible to the formatting gate.** They are not exempt from the project's
style rule — nothing ever decided that — they are simply unreachable by the
command that enforces it. So they drift, silently, for as long as nobody
happens to run `cargo fmt` inside one of those directories by hand.

Measured on 2026-08-12: seven excluded crates plus the fuzz harness had
unformatted code. `tools/difftest` — the one a stale roadmap item had named
as the offender — was clean. The item had been carried across sessions with a
specific figure ("109 diffs in tools/difftest") that was wrong in both halves.
That is the second-order reason this gate exists: a hole in coverage does not
merely let drift happen, it lets *wrong beliefs about the drift* persist,
because there is no command anyone can run to check.

WHAT IT CHECKS, AND WHY IT DERIVES ITS OWN LIST
===============================================

Two categories, and the second is the one a hard-coded list would miss:

1. Every path in the root manifest's `exclude` array that contains a
   `Cargo.toml`. Read from the manifest at run time rather than duplicated
   here — a list copied into this file would go stale the first time someone
   adds a sweep tool, which is exactly the failure this gate is meant to
   prevent, reproduced one level up.

2. Every directory under `tools/` and `fuzz/` holding a `Cargo.toml` that is
   NEITHER a workspace member NOR in the exclude list. Such a crate is
   invisible to *both* gates simultaneously — `cargo fmt --all` skips it
   because it is not a member, and this script would skip it if it only read
   `exclude`. It is reported by name, because "a crate nobody's tooling can
   see" is worth surfacing on its own, independent of its formatting.

EXIT CODES
==========

0 — every reachable excluded crate is formatted.
1 — at least one has unformatted code, or `cargo fmt` failed to run. The
    offending crates and their diff counts are printed; the remedy is
    `cd <crate> && cargo fmt`.

Deliberately NOT auto-fixing: a gate that silently rewrites source turns a
review signal into a no-op, and this one runs in CI where there is nobody to
review the rewrite.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def _array(manifest: str, key: str) -> list[str]:
    """Pull a simple string array out of the root manifest.

    A deliberately small reader rather than a TOML dependency: this gate must
    run on a bare checkout with nothing installed beyond Python and cargo, and
    the two arrays it needs (`members`, `exclude`) are plain lists of quoted
    strings in a file this project controls. If either ever grows inline
    tables or comments mid-array, this returns fewer entries and the gate
    reports crates as unlisted rather than passing them silently — a
    conservative failure, which is the right direction for a coverage check.
    """
    match = re.search(rf"^{key}\s*=\s*\[(.*?)\]", manifest, re.S | re.M)
    if not match:
        return []
    return re.findall(r'"([^"]+)"', match.group(1))


def _diff_count(crate: Path) -> tuple[int, str]:
    """Run `cargo fmt --check` in `crate`; return (diff hunks, error text)."""
    try:
        proc = subprocess.run(
            ["cargo", "fmt", "--check"],
            cwd=crate,
            capture_output=True,
            text=True,
            errors="replace",
        )
    except OSError as exc:
        # An OSError here means cargo could not be executed at all. Reported
        # as a failure rather than swallowed: a check that cannot run is not a
        # check that passed, and treating the two alike is how a harness comes
        # to report success it never measured (standing rule R191).
        return -1, f"could not run cargo: {exc}"
    if proc.returncode not in (0, 1):
        return -1, (proc.stderr or proc.stdout).strip()[:400]
    return len(re.findall(r"^Diff in ", proc.stdout, re.M)), ""


def main() -> int:
    manifest = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    members = _array(manifest, "members")
    excluded = _array(manifest, "exclude")

    known = {(ROOT / p).resolve() for p in members + excluded}

    targets: list[Path] = []
    for rel in excluded:
        crate = ROOT / rel
        if (crate / "Cargo.toml").is_file():
            targets.append(crate)

    # Category 2: crates listed in neither array. Invisible to both gates.
    unlisted: list[Path] = []
    for area in ("tools", "fuzz"):
        base = ROOT / area
        if not base.is_dir():
            continue
        if (base / "Cargo.toml").is_file() and base.resolve() not in known:
            unlisted.append(base)
        for child in sorted(base.iterdir()):
            if child.is_dir() and (child / "Cargo.toml").is_file():
                if child.resolve() not in known:
                    unlisted.append(child)
    targets.extend(unlisted)

    if not targets:
        print("fmt-excluded: no excluded crates found — nothing to check.")
        return 0

    failures: list[str] = []
    for crate in targets:
        count, err = _diff_count(crate)
        rel = crate.relative_to(ROOT).as_posix()
        if err:
            failures.append(f"  {rel}: {err}")
        elif count > 0:
            failures.append(f"  {rel}: {count} unformatted hunk(s)")

    if unlisted:
        print(
            "fmt-excluded: NOTE — crate(s) in neither `members` nor `exclude`; "
            "they are invisible to `cargo fmt --all` and to `cargo test`:"
        )
        for crate in unlisted:
            print(f"  {crate.relative_to(ROOT).as_posix()}")
        print()

    if failures:
        print("fmt-excluded: unformatted code outside the workspace.\n")
        print("\n".join(failures))
        print(
            "\n  These crates are excluded from the workspace deliberately "
            "(see Cargo.toml),\n  which means `cargo fmt --all --check` cannot "
            "reach them — it does NOT mean\n  they are exempt from project "
            "rule 10.\n\n  Fix: cd into each and run `cargo fmt`."
        )
        return 1

    print(f"fmt-excluded: clean — {len(targets)} out-of-workspace crate(s) checked.")
    return 0


if __name__ == "__main__":
    try:
        sys.stdout.reconfigure(encoding="utf-8")
        sys.stderr.reconfigure(encoding="utf-8")
    except AttributeError:
        pass
    sys.exit(main())
