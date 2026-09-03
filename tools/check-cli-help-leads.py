#!/usr/bin/env python3
"""check-cli-help-leads.py — a subcommand's `--help` summary must be the FIRST
line of its doc comment, not a paragraph buried inside it.

WHAT THIS GATE IS FOR
=====================

In `clap`-derive, a `///` doc comment **is** the shipped `--help` text, and
only its **first line** becomes the short summary shown in
`pdfcer --help`. Everything after it is long help nobody sees unless they
ask for it.

So a doc block whose real summary sits in the middle ships a `--help` line
that describes **a different command**. Nothing fails: the code compiles, the
tests pass, `cargo doc` renders it, and the operator reads the wrong sentence.

★ TWO REAL INSTANCES, BOTH FOUND BY EYE, BOTH PRE-EXISTING
==========================================================

Found 2026-08-29 while smoke-testing an unrelated feature — by running
`pdfcer --help` and reading it, which is not a thing any test did:

  * `fetch-ocr-models` began `/// Render a page to a PNG image.` — a stray
    first line belonging to `render-page`. Its shipped summary therefore
    said it renders a page. It downloads model weights.

  * `print` began with a paragraph reading *"**Report what printing this
    document WOULD do**, without printing"* — which is `print-preview`'s
    subject — while its own `**Send pages to a printer.**` sat eleven lines
    down. Its shipped summary claimed it does not print. `--send` prints.

The second is the more dangerous shape: the text was not wrong about
anything, it was **in the wrong place**, so a reader reviewing the diff that
introduced it would have seen accurate prose.

THE MECHANISM, WHICH IS WHY A GATE IS WORTH IT
==============================================

Both are the same failure as `R2xx`'s doc-orphaning: a new variant, or a new
paragraph, spliced at a line offset that landed **inside** the preceding
`///` block instead of after it. That is a mechanical error made by an editing
tool, not a judgement error made by an author, so it recurs and it is
mechanically detectable.

WHAT IT CHECKS
==============

This project's house style opens every subcommand doc with a **bold lead-in**
— `**Rasterise one page to a PNG**`, `**Send pages to a printer.**`. The
invariant that follows:

    a `///` line that STARTS a bold lead-in (`**`) must be the FIRST line of
    its doc block.

A bold lead-in appearing after a line that ended a sentence means a second
summary was spliced into somebody else's block. Bold used mid-sentence for
emphasis is unaffected (the line does not start with `**`), and a block whose
lead-in wraps onto a second line is unaffected (the previous line does not end
in a full stop).

Measured on the tree that introduced it: 2 candidates, 2 true positives, 0
false positives across 31,000 lines.

EXIT
====
0 — clean. 1 — at least one buried summary, listed with its file:line.

Run:  python tools/check-cli-help-leads.py
"""

import pathlib
import sys

TARGETS = [
    "crates/pdfcer-cli/src/main.rs",
]


def offenders(path: pathlib.Path):
    """Yield (line_number, previous_line, offending_line) for each buried lead."""
    lines = path.read_text(encoding="utf-8").split("\n")
    found = []
    for i in range(1, len(lines)):
        prev, cur = lines[i - 1].strip(), lines[i].strip()
        if not (prev.startswith("///") and cur.startswith("///")):
            continue
        prev_text, cur_text = prev[3:].strip(), cur[3:].strip()
        if not prev_text or not cur_text:
            continue
        # A bold lead-in that is not the first line of its block, following a
        # line that closed a sentence: a second summary landed inside somebody
        # else's doc comment.
        if cur_text.startswith("**") and not prev_text.startswith("**") and prev_text.endswith("."):
            found.append((i + 1, prev_text, cur_text))
    return found


def main() -> int:
    root = pathlib.Path(__file__).resolve().parent.parent
    bad = 0
    for rel in TARGETS:
        path = root / rel
        if not path.exists():
            print(f"check-cli-help-leads: MISSING {rel}", file=sys.stderr)
            return 1
        for line_no, prev, cur in offenders(path):
            bad += 1
            print(
                f"{rel}:{line_no}: a bold summary lead-in is buried inside a doc "
                f"comment, so clap ships the line ABOVE it as this subcommand's "
                f"--help summary.\n"
                f"    shipped as the summary (from further up the block): ...{prev}\n"
                f"    the real summary, invisible in --help:              {cur}",
                file=sys.stderr,
            )
    if bad:
        print(
            f"\ncheck-cli-help-leads: {bad} buried summary line(s). Move the "
            f"bold lead-in to the FIRST line of its own doc block -- in "
            f"clap-derive the first line IS the shipped --help text, and a "
            f"buried one means a paragraph was spliced into the preceding "
            f"variant's comment.",
            file=sys.stderr,
        )
        return 1
    print("check-cli-help-leads: OK -- every bold summary lead-in opens its own doc block.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
