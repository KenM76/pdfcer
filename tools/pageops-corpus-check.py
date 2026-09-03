#!/usr/bin/env python3
"""Sweep the structural page operations across a real-world PDF corpus.

WHY THIS EXISTS
---------------
`tools/roundtrip` proves the ARCHITECTURE.md §5 invariant for saves. It
does not exercise Pass 3.2's operations, which have their own failure
modes — a page-tree splice that leaves an inconsistent ``/Count``, a
free-list chain that a stricter reader rejects, a deep-copy closure that
either drags in the whole document or drops something a page needed.

Those are exactly the shapes that pass a fixture test and fail on a real
file, so this runs them over the corpus. It is deliberately a *sweep*,
not a proof: it asserts that every operation either **succeeds and
produces a document pdfcer can read back with the page count it claimed**,
or **refuses by name**. A refusal is a correct outcome (decision 007's
R27 posture), and a sweep that counted refusals as failures would push
the implementation toward guessing.

WHAT IT RUNS, PER FILE
----------------------
1. ``delete-pages --pages <last>`` then reload — the free-list path
   (decision 007 **W9**), plus the ancestor-``/Count`` recomputation.
   Skipped for single-page documents, where deleting the only page is a
   correct refusal.
2. ``extract-pages --pages 1`` — the deep-copy closure with its barrier,
   the outline subset walk, the name-tree flatten, the AcroForm widget
   census. This is the operation that touches the most machinery.

Both outputs are reloaded through ``inspect``, and the delete's page
count is checked against the source's minus one.

USAGE
-----
    python tools/pageops-corpus-check.py <corpus-dir> [--limit N]

Exit code 0 when there were no failures (refusals do not count), 1
otherwise. Every failure is printed with its file and its reason —
counted, never rounded away.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tempfile
from collections import Counter
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
CLI = REPO / "target" / "release" / "pdfcer.exe"
if not CLI.exists():
    CLI = REPO / "target" / "debug" / "pdfcer.exe"

# pdfcer's documented exit codes (see its `exit` module). Only these
# two are "the operation did not happen, and that is fine".
EXIT_OK = 0
EXIT_EDIT_REFUSED = 9
EXIT_SAVE_REFUSED = 8

PAGES_RE = re.compile(r"\bpages=(\d+)")


def run(args: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(CLI), *args], capture_output=True, text=True, errors="replace"
    )


def page_count(path: Path) -> int | None:
    """The document's page count, via a one-page extract's own report.

    Uses ``extract-pages --pages all``'s ``pages=`` metric rather than a
    dedicated subcommand, because the CLI has no "count the pages"
    command and inventing one for a test script would be adding public
    surface to satisfy a script.
    """
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / "all.pdf"
        result = run(
            ["extract-pages", str(path), "--pages", "all", "-o", str(out)]
        )
        if result.returncode != EXIT_OK:
            return None
        match = PAGES_RE.search(result.stdout)
        return int(match.group(1)) if match else None


def check_file(path: Path, tmp: Path, failures: list[str], tally: Counter) -> None:
    count = page_count(path)
    if count is None:
        tally["not-loadable-or-refused"] += 1
        return
    tally["files"] += 1

    # --- extract page 1 ------------------------------------------------
    extracted = tmp / "extract.pdf"
    result = run(["extract-pages", str(path), "--pages", "1", "-o", str(extracted)])
    if result.returncode == EXIT_OK:
        tally["extract-ok"] += 1
        if run(["inspect", str(extracted)]).returncode != EXIT_OK:
            failures.append(f"{path}: extracted file does not reload")
        got = PAGES_RE.search(result.stdout)
        if got and int(got.group(1)) != 1:
            failures.append(f"{path}: extract reported {got.group(1)} pages, wanted 1")
    elif result.returncode in (EXIT_EDIT_REFUSED, EXIT_SAVE_REFUSED):
        tally["extract-refused"] += 1
    else:
        failures.append(
            f"{path}: extract exited {result.returncode}: "
            f"{result.stderr.strip().splitlines()[:1]}"
        )

    # --- delete the last page ------------------------------------------
    if count < 2:
        tally["delete-skipped-single-page"] += 1
        return
    deleted = tmp / "deleted.pdf"
    result = run(
        [
            "delete-pages",
            str(path),
            "--pages",
            str(count),
            "-o",
            str(deleted),
        ]
    )
    if result.returncode == EXIT_OK:
        tally["delete-ok"] += 1
        after = page_count(deleted)
        if after is None:
            failures.append(f"{path}: file does not reload after delete")
        elif after != count - 1:
            failures.append(
                f"{path}: after deleting 1 of {count} pages the file has {after}"
            )
    elif result.returncode in (EXIT_EDIT_REFUSED, EXIT_SAVE_REFUSED):
        tally["delete-refused"] += 1
    else:
        failures.append(
            f"{path}: delete exited {result.returncode}: "
            f"{result.stderr.strip().splitlines()[:1]}"
        )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("corpus", type=Path)
    parser.add_argument("--limit", type=int, default=0, help="stop after N files")
    args = parser.parse_args()

    if not CLI.exists():
        print(f"pdfcer not built at {CLI}", file=sys.stderr)
        return 1

    pdfs = sorted(args.corpus.rglob("*.pdf"))
    if args.limit:
        pdfs = pdfs[: args.limit]
    print(f"{len(pdfs)} PDF file(s) under {args.corpus}")

    failures: list[str] = []
    tally: Counter = Counter()
    with tempfile.TemporaryDirectory() as tmpdir:
        tmp = Path(tmpdir)
        for index, path in enumerate(pdfs):
            check_file(path, tmp, failures, tally)
            if index % 200 == 0:
                print(f"  {index}/{len(pdfs)} …", flush=True)

    print("\n=== page-operations corpus sweep ===")
    for key in sorted(tally):
        print(f"{key:32} {tally[key]}")
    print(f"{'FAILURES':32} {len(failures)}")
    for line in failures[:50]:
        print(f"  {line}")
    if len(failures) > 50:
        print(f"  … and {len(failures) - 50} more")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
