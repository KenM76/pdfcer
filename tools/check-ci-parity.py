#!/usr/bin/env python3
"""check-ci-parity.py — every CI command is one somebody can run locally.

WHY THIS EXISTS
===============

On 2026-08-21, while verifying CI before tagging ``v0.7.0``, the
``fuzz targets build (nightly)`` job turned out to have been **red for three
days** on a one-line compile break: ``MarkupSpec::Square`` had gained a
``border_effect`` field and ``fuzz/fuzz_targets/annot_author.rs``, which
constructs that variant, had not been updated.

Nothing local contradicted it, and **that is the whole point of this
script.** The habit this project calls "run the gates" is::

    for g in tools/check-*; do ...; done

Fourteen scripts, all green, throughout. **CI runs nine jobs — ten runs,
since ``test`` is a two-OS matrix — and that sweep is ONE of them.**
``cargo fuzz build``, the wasm32 cross-target check, the no-network
denylist and the third-party licence audit have **no local equivalent
anybody runs by habit**, so a green local sweep and a red CI were never a
contradiction and the operator had no way to know that from either signal.

★ **The instance was cheap; the class is not.** Fixing the fuzz target
closes one hole. What this script closes is the *shape*: a CI job added
tomorrow, with no local runner, would be exactly as invisible.

WHAT IT CHECKS
==============

Every ``run:`` command in ``.github/workflows/*.yml`` is classified against
the table below into one of three states:

``LOCAL``
    A ``tools/check-*`` script, or a plain ``cargo`` command the engineer
    runs anyway. Running the local sweep plus ``cargo test``/``fmt``/
    ``clippy`` covers it.

``LOCAL-VIA``
    Not run by the sweep, but there **is** a documented local command that
    catches the same failure class. Recorded with that command, so a human
    can run it and so this file answers "what would have caught this?".

``CI-ONLY``
    Genuinely unrunnable or impractical locally — a matrix leg for another
    OS, a toolchain install. Listed by name so the set stays small and
    deliberate rather than growing by default.

An unrecognised command is a **failure**. That is the entire mechanism: a
new CI job cannot be added without someone deciding, in this file, whether
a local runner exists.

WHAT IT DELIBERATELY DOES NOT DO
================================

It does not *run* anything. Two reasons. A parity checker that ran the jobs
would take the fifteen minutes CI takes, so nobody would run it — and the
question it answers ("is anything unaccounted for?") is a property of the
workflow file, not of today's tree.

It also does not check that a ``LOCAL-VIA`` command *passes*. It checks that
one **exists and is written down**. Whether it passes is the job of running
it, and ``COMMANDS`` below is the list of what to run.

USAGE
=====

::

    python tools/check-ci-parity.py          # audit
    python tools/check-ci-parity.py --list   # print the local command list
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
WORKFLOWS = ROOT / ".github" / "workflows"

# --------------------------------------------------------------------------
# The classification table.
#
# Keys are SUBSTRINGS matched against a normalised (whitespace-collapsed)
# CI command. Substrings rather than exact strings on purpose: CI commands
# carry `${{ matrix.os }}` interpolations and shell continuations, and an
# exact-match table would go stale on a whitespace edit — which is the
# stale-anchor failure `tools/check-ledger-numbers.py` has now paid for
# three times.
# --------------------------------------------------------------------------
LOCAL = {
    "cargo fmt --all --check": "cargo fmt --all --check",
    "tools/check-fmt-excluded.py": "python tools/check-fmt-excluded.py",
    "tools/check-shipped-assets.py": "python tools/check-shipped-assets.py",
    "cargo clippy": "cargo clippy --workspace --all-targets --all-features -- -D warnings",
    "cargo test --workspace": "cargo test --workspace --all-features",
    "cargo test -p pdfce-core --no-default-features": (
        "cargo test -p pdfce-core --no-default-features"
    ),
    "tools/check-outcome-disclosed.py": "python tools/check-outcome-disclosed.py",
    "tools/check-commits-filed.py": "python tools/check-commits-filed.py",
    "tools/check-bypass-paths.sh": "bash tools/check-bypass-paths.sh",
    "tools/check-core-api-verbs.py": "python tools/check-core-api-verbs.py",
    "tools/check-clap-help.py": "python tools/check-clap-help.py",
    "tools/check-cited-verbs-exist.py": "python tools/check-cited-verbs-exist.py",
    "tools/check-ledger-numbers.py": "python tools/check-ledger-numbers.py",
    "tools/check-metrics-line-contract.py": "python tools/check-metrics-line-contract.py",
    "tools/check-one-commit-per-command.py": "python tools/check-one-commit-per-command.py",
    "tools/check-cli-help-leads.py": "python tools/check-cli-help-leads.py",
    "tools/check-passes-filed.py": "python tools/check-passes-filed.py",
    "tools/check-settings-consumed.py": "python tools/check-settings-consumed.py",
    "tools/check-suite-name-absent.py": "python tools/check-suite-name-absent.py",
    "tools/check-control-bytes.py": "python tools/check-control-bytes.py",
    "tools/check-string-gaps.sh": "bash tools/check-string-gaps.sh",
    "tools/check-public-fns-documented.py": "python tools/check-public-fns-documented.py",
    "tools/check-cited-commits-exist.py": "python tools/check-cited-commits-exist.py",
    "tools/check-ci-parity.py": "python tools/check-ci-parity.py",
    "tools/check-ci-job-names.py": "python tools/check-ci-job-names.py",
}

# Not in the sweep, but a local command catches the same failure class.
# The comment on each is WHY the local stand-in is adequate, because a
# stand-in nobody trusts is a stand-in nobody runs.
LOCAL_VIA = {
    # `cargo fuzz build` needs nightly + ASan, and on Windows it needs the
    # MSVC `clang_rt.asan` DLL on PATH or it dies with STATUS_DLL_NOT_FOUND.
    # `cargo check --bins` inside `fuzz/` needs none of that and catches the
    # ENTIRE class that has actually broken this job: a fuzz target that no
    # longer compiles against the spec it constructs. It does not catch a
    # sanitizer-level problem — and no sanitizer-level problem has ever
    # broken this job.
    "cargo +nightly fuzz build": "cd fuzz && cargo check --bins",
    # The wasm32 leg is the enforcement of the web-fork invariant, and it
    # runs locally if the target is installed. `rustup target add
    # wasm32-unknown-unknown` once, then this is a normal cargo check.
    "--target wasm32-unknown-unknown": (
        "cargo check -p pdfce-core -p pdfce-render --target wasm32-unknown-unknown"
    ),
    # `cargo about` is installed on this machine (0.9.1). Regenerating and
    # diffing is what the audit does.
    "cargo about generate": "cargo about generate about.hbs -o THIRD_PARTY_LICENSES.md",
    # The GUI-dependency and network denylists are `cargo tree` greps and run
    # anywhere.
    #
    # ★ THE EXIT SENSE IS INVERTED FROM THE GREP, AND THAT IS NOT A DETAIL.
    # Corrected 2026-08-27, when `tools/run-gates.sh` ran this list for the
    # first time and reported it as the one FAILING command on a tree whose
    # dependency graph was clean.
    #
    # In CI the step reads `if cargo tree | grep …; then error; exit 1; fi` —
    # a MATCH is the failure. Flattened to a bare pipeline for local use, the
    # exit code flips: `grep` exits 1 when it finds nothing, which is exactly
    # the passing condition. So the command this file printed reported
    # **failure on a healthy tree and success on a violated one** — the worse
    # direction of the two, since anybody who ran it and saw nothing would
    # take the silence for a pass.
    #
    # It went unnoticed because nothing ran the list; it was read by humans and
    # retyped. `--list` is now consumed by a script, which is what turned a
    # documentation defect into a red line on the first run.
    #
    # `!` inverts it, and both crates are checked because CI checks both — a
    # local stand-in that covers half of a two-crate invariant is a stand-in
    # that can be green while CI is red.
    "cargo tree": (
        "! cargo tree -p pdfce-core -p pdfce-render 2>/dev/null "
        "| grep -Ei '(^|[[:space:]])(egui|eframe|winit|wgpu|reqwest|hyper)([[:space:]]|$)'"
    ),
}

# Genuinely CI-only. Keep this set SMALL and each entry justified — every
# line here is a thing no local run can tell you about.
CI_ONLY = {
    # A different operating system. Nothing local substitutes for a real
    # macOS/Linux compile; that is the job's entire purpose.
    "--target aarch64-apple-darwin": "cross-compiles for another OS",
    # Toolchain installation steps, not checks.
    "cargo install": "installs a tool; not itself a check",
    "rustup": "toolchain management",
    "apt-get": "runner provisioning",
}


def commands_in(path: Path) -> list[tuple[int, str]]:
    """Every `run:` command in a workflow, with its line number.

    Handles both the inline form (``- run: cmd``) and the block form
    (``run: |`` followed by an indented body). The block form is returned
    as ONE entry containing its whole body, because a block is one CI step
    and classifying its lines separately would report a `grep` inside a
    denylist as an unaccounted-for command.
    """
    out: list[tuple[int, str]] = []
    lines = path.read_text(encoding="utf-8").splitlines()
    i = 0
    while i < len(lines):
        m = re.match(r"^(\s*)-?\s*run:\s*(\|.*)?$", lines[i])
        if m and m.group(2) is not None:
            indent = len(m.group(1))
            body: list[str] = []
            j = i + 1
            while j < len(lines):
                if lines[j].strip() and (len(lines[j]) - len(lines[j].lstrip())) <= indent:
                    break
                body.append(lines[j].strip())
                j += 1
            out.append((i + 1, " ".join(body)))
            i = j
            continue
        m = re.match(r"^\s*-?\s*run:\s+(\S.*)$", lines[i])
        if m:
            out.append((i + 1, m.group(1).strip()))
        i += 1
    return out


def classify(cmd: str) -> tuple[str, str] | None:
    for needle, local in LOCAL.items():
        if needle in cmd:
            return ("LOCAL", local)
    for needle, local in LOCAL_VIA.items():
        if needle in cmd:
            return ("LOCAL-VIA", local)
    for needle, why in CI_ONLY.items():
        if needle in cmd:
            return ("CI-ONLY", why)
    return None


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--list",
        action="store_true",
        help="print the local command list and exit",
    )
    args = ap.parse_args()

    if args.list:
        print("Run these locally to cover what CI covers:\n")
        for c in sorted(set(LOCAL.values()) | set(LOCAL_VIA.values())):
            print(f"  {c}")
        return 0

    if not WORKFLOWS.is_dir():
        print(f"ci-parity: no {WORKFLOWS} — nothing to audit.")
        return 0

    unaccounted: list[str] = []
    counts = {"LOCAL": 0, "LOCAL-VIA": 0, "CI-ONLY": 0}
    for wf in sorted(WORKFLOWS.glob("*.yml")):
        for line, cmd in commands_in(wf):
            verdict = classify(cmd)
            if verdict is None:
                unaccounted.append(f"{wf.relative_to(ROOT)}:{line}  {cmd[:110]}")
            else:
                counts[verdict[0]] += 1

    if unaccounted:
        print("ci-parity: CI runs command(s) with no local story.\n")
        for u in unaccounted:
            print(f"  {u}")
        print(
            "\n  Decide, in tools/check-ci-parity.py, which of the three each is:\n"
            "    LOCAL      — the gate sweep or a cargo command already covers it\n"
            "    LOCAL-VIA  — a DIFFERENT local command catches the same class;\n"
            "                 write that command down beside it\n"
            "    CI-ONLY    — genuinely unrunnable here; say why\n"
            "\n  This exists because `cargo fuzz build` was red for three days\n"
            "  while every local check was green, and neither signal was wrong."
        )
        return 1

    print(
        f"ci-parity: clean — {counts['LOCAL']} covered by the local sweep, "
        f"{counts['LOCAL-VIA']} by a named local stand-in, "
        f"{counts['CI-ONLY']} genuinely CI-only."
    )
    print("  `--list` prints what to run locally to match CI.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
