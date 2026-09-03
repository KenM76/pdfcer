#!/usr/bin/env python3
"""Fail if ``self.commit(`` is called from inside a loop.

WHY THIS EXISTS
===============

Standing rule **R179** — *a loop that mutates as it goes must not ``?`` a
per-item refusal*. The rule was minted after ``import_form_data`` was found
propagating a per-entry error out of a loop that had already changed the
document: the call reported failure, and the file was modified anyway.

That defect was fixed three times before it was fixed once. The first fix
handled the rich-text arm only; a later audit found the button and choice
arms doing the same thing (``b2574f6``). Two further loops were then run
down by hand: the redaction search engine, which turned out to be safe for
reasons entirely outside the function (``079394f``), and ``cmd_fill_field``,
safe because its early return happens before the save.

A FULL AUDIT of ``edit.rs`` (2026-08-10) found **17** loops containing a
``?`` on a ``self.`` call. Fifteen were harmless, and all fifteen for the
SAME structural reason:

    the helper BUILDS an ``ObjectWrite`` and returns it;
    the loop accumulates into a ``writes`` vector;
    the caller commits ONCE, after the loop.

``set_widget_as``, ``set_widget_ap``, ``rotation_write`` and
``retarget_annot`` all read like mutations from their names and are not —
none of them calls ``self.commit``. That convention is what makes pdfcer
resistant to R179's whole class of defect, and it was holding by habit,
enforced by nothing.

The failure mode this gate exists for is therefore NOT the ``?``. It is a
future helper that commits inside a loop. The moment one does:

* a single operator gesture becomes N undoable commands, so undo peels the
  operation apart one item at a time through states nobody asked for
  (project rule: one gesture, one undo — R49);
* and a mid-loop failure leaves the earlier commits standing, which is
  exactly the ``import_form_data`` defect wearing different clothes.

Checking for the ``?`` directly would be useless: fifteen of seventeen
sites are legitimate and a gate that cries wolf fifteen times is a gate
people learn to skip. Checking for the COMMIT is precise, currently green
with zero exemptions, and catches the thing that actually goes wrong.

WHAT IT CHECKS
==============

For every Rust file under ``crates/`` (``edit.rs`` is where the commands
live today, but the invariant is not file-specific and a future split must
not silently escape it):

1. Find each ``for``/``while`` loop header that opens a brace on its own
   line.
2. Track brace depth to find that loop's body.
3. Report any ``self.commit(`` inside it.

Nested loops are covered by construction — an inner loop's body is a
subset of the outer's.

WHAT IT DELIBERATELY DOES NOT CHECK
===================================

*Closures* that commit. ``.for_each(|x| self.commit(...))`` would evade
this, and a regex-and-brace-depth scanner cannot reliably tell a closure
that runs once from one that runs per item. Recognising that limit is
better than pretending to cover it: this gate is a tripwire on the common
shape, not a proof. A real proof needs the compiler, and the honest place
for that is a lint, not a script.

It also does not check the ``?``. See above — the false-positive rate is
what would kill it.

EXIT CODES
==========

``0`` — no ``self.commit(`` inside any loop.
``1`` — at least one found; each is printed with file, line and the loop
        header it sits inside.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

# Windows consoles default to a code page that cannot encode the em-dashes,
# arrows and stars this file prints, so Python substitutes "?" for exactly
# the characters that make a failure message readable. One reconfigure fixes
# every message in the file without flattening the typography.
#
# This is not theoretical: `check-commits-filed.py` was observed printing
# "each commit's full message ? they carry" while doing its job correctly.
# Found by reading a gate's output as its audience (R174), not by reading
# its source.
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")


REPO = Path(__file__).resolve().parent.parent
CRATES = REPO / "crates"

LOOP_HEADER = re.compile(r"^\s*(?:\}\s*)?(for|while)\b.*\{\s*$")
COMMIT = re.compile(r"\bself\.commit\s*\(")


def scan(path: Path) -> list[tuple[int, str, int, str]]:
    """Return (loop_line, loop_text, commit_line, commit_text) for each hit."""
    try:
        lines = path.read_text(encoding="utf-8", errors="replace").split("\n")
    except OSError:
        return []

    hits: list[tuple[int, str, int, str]] = []
    for i, header in enumerate(lines):
        if not LOOP_HEADER.match(header):
            continue
        # Walk the body by brace depth. The header itself opens one level;
        # the body ends when depth returns to zero.
        depth = header.count("{") - header.count("}")
        j = i + 1
        while j < len(lines) and depth > 0:
            if COMMIT.search(lines[j]):
                hits.append((i + 1, header.strip(), j + 1, lines[j].strip()))
            depth += lines[j].count("{") - lines[j].count("}")
            j += 1
    return hits


def main() -> int:
    findings: list[tuple[Path, int, str, int, str]] = []
    for rs in sorted(CRATES.rglob("*.rs")):
        for loop_line, loop_text, commit_line, commit_text in scan(rs):
            findings.append((rs, loop_line, loop_text, commit_line, commit_text))

    if not findings:
        print("one-commit-per-command: no self.commit() inside a loop.")
        return 0

    print(f"one-commit-per-command: {len(findings)} commit(s) inside a loop.\n")
    for path, loop_line, loop_text, commit_line, commit_text in findings:
        rel = path.relative_to(REPO).as_posix()
        print(f"  {rel}:{commit_line}")
        print(f"    {commit_text}")
        print(f"  inside the loop opened at {rel}:{loop_line}")
        print(f"    {loop_text}\n")

    print(
        "  R179 / R49. A command must commit ONCE. Build the writes in the\n"
        "  loop, accumulate them, and commit after it — the shape every\n"
        "  other multi-object verb in this crate already uses.\n\n"
        "  Committing per iteration makes one operator gesture into N undo\n"
        "  entries, and leaves earlier commits standing when a later\n"
        "  iteration fails. That second half is the `import_form_data`\n"
        "  defect exactly.\n\n"
        "  If this is genuinely a loop that runs once, hoist the commit out\n"
        "  of it rather than exempting it here."
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
