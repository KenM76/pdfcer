#!/usr/bin/env python3
"""Sweep text extraction across a real-world PDF corpus, and report the
§9.10.2 ladder statistics honestly.

WHY THIS EXISTS
---------------
Pass 4's fixtures are worked examples from specific ISO 32000-1 clauses.
They prove that each rung of the ladder does what the clause says. They
prove **nothing** about the question an operator actually asks:

    "If I hit Copy on a real document, how much of what I get is what
    the file says, and how much did pdfcer make up?"

That number cannot be asserted from a fixture; it has to be measured.
This sweep runs ``pdfcer extract-text`` over every PDF in a corpus
directory and rolls up the per-rung counters, so the answer is a
measurement with a denominator rather than an impression.

It is also the panic gate. Extraction walks content streams, decodes
CMaps and resolves fonts on files pdfcer has never seen — the same class
of adversarial-input surface the fuzz targets cover, but with real
structural variety behind it. **Any panic here is a pdfcer bug**, and the
sweep exits non-zero for one.

WHAT IT MEASURES, PER FILE
--------------------------
``extract-text --pages all`` and the ``key=value`` result line it emits
on stderr (the stable R5 contract line — see ``cmd_extract_text``'s
docs for the channel routing). Rolled up:

* ``codes`` — character codes taken off show strings, the denominator
  for everything else.
* ``via_tounicode`` / ``via_encoding`` / ``via_cid`` — §9.10.2 rungs 1,
  2 and 3. **SOURCED**: ISO 32000-1 itself sanctions these values.
* ``via_extension`` — pdfcer's counted glyph-name extension (the font
  failed method 2's whole-array precondition but the name resolved
  through the AGL anyway). Recovered text, **not** sourced.
* ``failed`` — codes that reached §9.10.2's failure clause and became
  U+FFFD. The headline honesty metric.
* ``identity_no_tounicode`` — fonts for which "no Unicode is
  recoverable" is the *standard's* answer, not a pdfcer limitation.
* ``spaces_derived`` / ``lines_derived`` — whitespace pdfcer invented.
* ``tagged`` — how many documents carry ``/MarkInfo /Marked true``, i.e.
  for how many of them §14.8.1's four guarantees hold at all.

WHAT COUNTS AS A FAILURE
------------------------
Only two things: a **panic** (exit 101 or a panic message on stderr),
and a **timeout**. Everything else is a legitimate outcome and is
tallied rather than judged:

* exit 3/4 — the file is not readable as a PDF. That is a *loader*
  result, not an extraction result; the corpus deliberately contains
  16 ``*-fail-*`` conformance files.
* exit 1 — a page tree that will not walk, or an encrypted document
  (Pass 5). Counted as ``skipped``.
* 100% ``failed`` codes — a correct, honest outcome for an
  ``Identity-H`` font with no ``/ToUnicode``. Counting it as a defect
  would push the implementation toward guessing, which is precisely
  what Pass 4 exists not to do.

USAGE
-----
    python tools/text-corpus-check.py <corpus-dir> [--limit N] [--tsv PATH]

Exit 0 when there were no panics and no timeouts, 1 otherwise.
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from collections import Counter
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

CLI = REPO / "target" / "release" / "pdfcer.exe"
if not CLI.exists():
    CLI = REPO / "target" / "debug" / "pdfcer.exe"
if not CLI.exists():
    CLI = REPO / "target" / "release" / "pdfcer"
if not CLI.exists():
    CLI = REPO / "target" / "debug" / "pdfcer"

# Per-file wall-clock budget. Extraction is linear in content-stream
# length with bounded per-font work, so anything past this is a hang, not
# a big file — and a hang is a finding.
FILE_BUDGET_SECONDS = 30

# The counters rolled up from the result line, in report order.
COUNTERS = [
    "codes",
    "via_tounicode",
    "via_encoding",
    "via_cid",
    "via_extension",
    "failed",
    "spaces_derived",
    "lines_derived",
    "actual_text",
    "artifacts",
    "reversed",
    "identity_no_tounicode",
    "ucs2_missing",
    "predefined_cmaps_missing",
    "forms",
    "rtl_runs",
    "invisible",
    "unreadable_pages",
]

# Flags counted as documents, not as occurrences.
FLAGS = ["tagged", "suspects", "struct_tree"]


def parse_result_line(text: str) -> dict[str, str] | None:
    """Pull the stable ``key=value`` result line out of a stream."""
    for line in text.splitlines():
        if line.startswith("extracted "):
            pairs = {}
            for token in line.split():
                if "=" in token:
                    key, _, value = token.partition("=")
                    pairs[key] = value
            return pairs
    return None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("corpus", type=Path)
    parser.add_argument("--limit", type=int, default=0)
    parser.add_argument("--tsv", type=Path, default=None)
    args = parser.parse_args()

    if not CLI.exists():
        print(f"pdfcer not built at {CLI}", file=sys.stderr)
        return 2

    files = sorted(args.corpus.rglob("*.pdf"))
    if args.limit:
        files = files[: args.limit]
    if not files:
        print(f"no PDFs under {args.corpus}", file=sys.stderr)
        return 2

    totals: Counter[str] = Counter()
    outcomes: Counter[str] = Counter()
    panics: list[str] = []
    timeouts: list[str] = []
    # Files whose codes ALL failed the ladder — not a defect, but the
    # single most useful list to look at afterwards.
    fully_unrecoverable: list[str] = []
    rows: list[str] = []

    for index, path in enumerate(files, 1):
        if index % 250 == 0:
            print(f"  ... {index}/{len(files)}", file=sys.stderr)
        try:
            proc = subprocess.run(
                [str(CLI), "extract-text", str(path)],
                capture_output=True,
                text=True,
                errors="replace",
                timeout=FILE_BUDGET_SECONDS,
            )
        except subprocess.TimeoutExpired:
            timeouts.append(str(path))
            outcomes["timeout"] += 1
            continue

        combined = proc.stderr + proc.stdout
        if proc.returncode == 101 or "panicked at" in combined:
            panics.append(str(path))
            outcomes["PANIC"] += 1
            continue

        if proc.returncode in (3, 4):
            outcomes["not-loadable"] += 1
            continue
        if proc.returncode != 0:
            outcomes[f"skipped (exit {proc.returncode})"] += 1
            continue

        kv = parse_result_line(proc.stderr) or parse_result_line(proc.stdout)
        if kv is None:
            outcomes["no result line"] += 1
            continue

        outcomes["extracted"] += 1
        for name in COUNTERS:
            try:
                totals[name] += int(kv.get(name, "0"))
            except ValueError:
                pass
        for flag in FLAGS:
            if kv.get(flag) == "true":
                totals[f"docs_{flag}"] += 1

        codes = int(kv.get("codes", "0") or 0)
        failed = int(kv.get("failed", "0") or 0)
        if codes > 0 and failed == codes:
            fully_unrecoverable.append(str(path))
        if codes > 0:
            totals["docs_with_text"] += 1

        if args.tsv:
            rel = path.relative_to(args.corpus).as_posix()
            rows.append(
                "\t".join([rel] + [kv.get(name, "0") for name in COUNTERS])
            )

    # ---------------------------------------------------------------
    # Report
    # ---------------------------------------------------------------
    print()
    print(f"corpus: {args.corpus}  files: {len(files)}")
    print()
    print("outcome                       count")
    print("-" * 44)
    for name, count in sorted(outcomes.items(), key=lambda kv: -kv[1]):
        print(f"{name:<28}{count:>8}")

    codes = totals["codes"]
    print()
    print(f"documents with any text: {totals['docs_with_text']}")
    print(f"character codes seen:    {codes}")
    if codes:
        sourced = totals["via_tounicode"] + totals["via_encoding"] + totals["via_cid"]
        print()
        print("ISO 32000-1 §9.10.2 ladder            codes        share")
        print("-" * 56)
        for label, key in [
            ("rung 1  /ToUnicode          SOURCED", "via_tounicode"),
            ("rung 2  encoding + AGL      SOURCED", "via_encoding"),
            ("rung 3  CID collection      SOURCED", "via_cid"),
            ("        pdfcer glyph-name extension  ", "via_extension"),
            ("rung 4  FAILED -> U+FFFD           ", "failed"),
        ]:
            value = totals[key]
            print(f"{label:<38}{value:>10}{value / codes:>11.2%}")
        print("-" * 56)
        print(f"{'SOURCED total':<38}{sourced:>10}{sourced / codes:>11.2%}")

    print()
    print("derived (pdfcer's own judgement, no spec basis)")
    print(f"  word spaces derived : {totals['spaces_derived']}")
    print(f"  line breaks derived : {totals['lines_derived']}")

    print()
    print("named gaps and mechanisms")
    for label, key in [
        ("fonts: Identity-H, no /ToUnicode", "identity_no_tounicode"),
        ("fonts: *-UCS2 CMap not bundled  ", "ucs2_missing"),
        ("fonts: predefined CMap not bundled", "predefined_cmaps_missing"),
        ("/ActualText replacements applied", "actual_text"),
        ("/Artifact sequences             ", "artifacts"),
        ("/ReversedChars sequences        ", "reversed"),
        ("form XObjects executed          ", "forms"),
        ("runs with RTL characters        ", "rtl_runs"),
        ("invisible glyphs (Tr 3 / Tr 7)  ", "invisible"),
        ("pages unreadable                ", "unreadable_pages"),
    ]:
        print(f"  {label}: {totals[key]}")

    print()
    print("document-level facts (§14.8.1)")
    print(f"  /MarkInfo /Marked true : {totals['docs_tagged']}")
    print(f"  /Suspects true         : {totals['docs_suspects']}")
    print(f"  /StructTreeRoot present: {totals['docs_struct_tree']}")

    if fully_unrecoverable:
        print()
        print(
            f"files where EVERY code failed the ladder: {len(fully_unrecoverable)} "
            "(an honest outcome, not a defect — see the module docs)"
        )
        for name in fully_unrecoverable[:10]:
            print(f"  {name}")
        if len(fully_unrecoverable) > 10:
            print(f"  ... and {len(fully_unrecoverable) - 10} more")

    if args.tsv:
        header = "\t".join(["file"] + COUNTERS)
        args.tsv.write_text("\n".join([header] + rows) + "\n", encoding="utf-8")
        print(f"\nwrote {args.tsv}")

    if panics:
        print()
        print(f"PANICS ({len(panics)}) — these are pdfcer BUGS:")
        for name in panics:
            print(f"  {name}")
    if timeouts:
        print()
        print(f"TIMEOUTS ({len(timeouts)}):")
        for name in timeouts:
            print(f"  {name}")

    return 1 if (panics or timeouts) else 0


if __name__ == "__main__":
    sys.exit(main())
