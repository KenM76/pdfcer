#!/usr/bin/env python3
"""Refuse a register entry that has grown into an essay.

WHY THIS EXISTS
===============

On 2026-09-10 the operator said the project *"has slowed to a crawl"* and
named the cause: the registers, not the code. Measured that morning:

    docs/ROADMAP.md      168,036 lines   (read every session)
    docs/SESSION_LOG.md   99,597 lines
    docs/ARCHITECTURE.md  34,340 lines
    docs/  (all)         372,011 lines   against 455,626 lines of crates/

and one day's work had added **2,722 lines of register against 5,844 lines of
code** — one line of bookkeeping per two lines of work, every one written by
hand at frontier-model rates, and the same paragraph landing four times over
(commit message, ROADMAP, SESSION_LOG, FEATURES).

The history was archived to `docs/history/` the same day. **That is a one-off
cure; this gate is the vaccine.** An archive with no cap refills.

WHAT IT CHECKS
==============

Four caps, each on a single entry in a live register:

    ROADMAP.md   `### ` entry under `## Shipped`      <= 150 lines
    ROADMAP.md   `### ` item under `## Next up`       <= 80 lines
    SESSION_LOG.md  `## ` filing entry                 <= 200 lines
    FEATURES.md  one table row                         <= 1,200 characters

The numbers are deliberately generous — roughly the 60th percentile of what
was already there — because the target is the ESSAY, not the paragraph. An
entry that needs more room is telling you its reasoning belongs in the commit
message, which is exhaustive, permanent, and read by nobody who is not looking
for it. **The register is an index into the record, not a second copy of it.**

WHAT IT DOES NOT CHECK
======================

`docs/history/**` — nothing there is on any read path, and re-editing an
archive to satisfy a cap would be pure churn.

`ARCHITECTURE.md` — its decision records are the reasoning of record and the
one place long-form argument is meant to live. It is 34,340 lines and that is
a separate conversation.

THE BASELINE
============

`tools/register-entry-size-baseline.txt` carries entries that were already
over the cap when the gate was written, one per line, as
`file:heading`. **It is DEBT, not an allowlist**: the gate prints the count
every run and the intended direction is down. A NEW entry over the cap is a
hard failure with no way to add it to the baseline except by hand, which is
the friction that makes trimming the cheaper option.

USAGE
=====

    python tools/check-register-entry-size.py [--stats] [--write-baseline]

Exit 0 clean, 1 on a finding.
"""

from __future__ import annotations

import io
import pathlib
import re
import sys

# Windows' console encoding is cp1252 and these registers are full of em
# dashes, stars and minus signs. A gate that crashes on its own findings
# reports nothing, which is the failure mode every gate here exists to avoid.
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")

ROOT = pathlib.Path(__file__).resolve().parent.parent
DOCS = ROOT / "docs"
BASELINE = ROOT / "tools" / "register-entry-size-baseline.txt"

SHIPPED_MAX = 150
NEXTUP_MAX = 80
SESSION_MAX = 200
FEATURES_ROW_MAX = 1200


def read(path: pathlib.Path) -> list[str]:
    return path.read_text(encoding="utf-8", errors="replace").split("\n")


def sections(lines: list[str], level: str) -> list[tuple[int, str, int]]:
    """(start_index, heading_text, length_in_lines) for each `level` heading."""
    idx = [i for i, l in enumerate(lines) if l.startswith(level)]
    out = []
    for n, i in enumerate(idx):
        end = idx[n + 1] if n + 1 < len(idx) else len(lines)
        out.append((i, lines[i].strip(), end - i))
    return out


def roadmap_findings() -> list[tuple[str, str, int, int]]:
    path = DOCS / "ROADMAP.md"
    lines = read(path)
    out = []
    bounds = {}
    for name in ("## Shipped", "## Next up"):
        start = next((i for i, l in enumerate(lines) if l.strip() == name), None)
        if start is None:
            continue
        after = [
            i
            for i, l in enumerate(lines)
            if l.startswith("## ") and i > start
        ]
        bounds[name] = (start, after[0] if after else len(lines))
    for name, cap in (("## Shipped", SHIPPED_MAX), ("## Next up", NEXTUP_MAX)):
        if name not in bounds:
            continue
        lo, hi = bounds[name]
        for i, head, length in sections(lines[lo:hi], "### "):
            if length > cap:
                out.append(("docs/ROADMAP.md", head, length, cap))
    return out


def session_findings() -> list[tuple[str, str, int, int]]:
    path = DOCS / "SESSION_LOG.md"
    lines = read(path)
    return [
        ("docs/SESSION_LOG.md", head, length, SESSION_MAX)
        for _, head, length in sections(lines, "## 20")
        if length > SESSION_MAX
    ]


def features_findings() -> list[tuple[str, str, int, int]]:
    path = DOCS / "FEATURES.md"
    out = []
    for line in read(path):
        if line.startswith("| [") and len(line) > FEATURES_ROW_MAX:
            # The row's own first distinguishing words make a stable key.
            cells = [c.strip() for c in line.split("|")]
            label = (cells[5] if len(cells) > 5 else line)[:70]
            out.append(("docs/FEATURES.md", label, len(line), FEATURES_ROW_MAX))
    return out


def key(finding) -> str:
    path, head, _, _ = finding
    # `.strip()` is load-bearing: the baseline file is read line-by-line with
    # `.strip()`, so a key with trailing whitespace -- and register headings
    # have plenty -- would never match itself. Cost one confused diagnosis.
    flat = re.sub(r"\s+", " ", head)[:120].strip()
    return f"{path}:{flat}"


def main() -> int:
    findings = roadmap_findings() + session_findings() + features_findings()
    if "--write-baseline" in sys.argv:
        BASELINE.write_text(
            "# Register entries already over the cap when"
            " check-register-entry-size.py was written (2026-09-10).\n"
            "# DEBT, not an allowlist. The intended direction is DOWN.\n"
            + "\n".join(sorted(key(f) for f in findings))
            + "\n",
            encoding="utf-8",
        )
        print(f"check-register-entry-size: baseline written, {len(findings)} entries")
        return 0

    baseline = set()
    if BASELINE.exists():
        baseline = {
            l.strip()
            for l in BASELINE.read_text(encoding="utf-8").split("\n")
            if l.strip() and not l.startswith("#")
        }

    new = [f for f in findings if key(f) not in baseline]
    carried = len(findings) - len(new)

    if "--stats" in sys.argv:
        print(f"  entries over cap        : {len(findings)}")
        print(f"  carried in the baseline : {carried}")

    if new:
        print("check-register-entry-size: FINDINGS —")
        for path, head, size, cap in new:
            unit = "chars" if path.endswith("FEATURES.md") else "lines"
            print(f"  {path}: {size} {unit} (cap {cap})")
            print(f"      {head[:110]}")
        print()
        print(
            "  A register entry is an INDEX INTO the record, not a second copy of"
            " it.\n"
            "  The reasoning belongs in the commit message: exhaustive, permanent,"
            " and\n"
            "  not on anybody's read-every-session list. Trim the entry to a"
            " verdict\n"
            "  plus a paragraph and cite the hash."
        )
        return 1

    print(
        f"check-register-entry-size: clean — {carried} over-cap entry(ies) carried"
        " in the baseline as DEBT"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
