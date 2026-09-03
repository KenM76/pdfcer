#!/usr/bin/env python3
"""Every `pdfcer` subcommand must carry operator-facing help text.

WHY THIS GATE EXISTS
====================

`clap` derives a subcommand's `--help` description from the Rust doc comment
on its `Command` enum variant. A variant with no doc comment therefore ships
a **blank description** -- in `pdfcer --help`'s subcommand list, and as an
empty first line of `pdfcer <sub> --help`. Nothing fails. The build is
clean, `clippy` is clean, `missing_docs` does not apply (these are private
items in a binary crate), and every test passes, because no test reads help
text.

This is the operator-facing half of a defect class this project has now hit
**eight times**: a doc comment that ends up attached to the wrong item, or to
no item at all.

  * Five instances in `crates/pdfcer-core/src/edit.rs` were splices that
    anchored on `pub fn name(` and landed INSIDE the preceding item's doc
    block, welding two blocks together. The recorded remedy -- insert AFTER a
    closing brace -- addresses the cause.
  * The sixth was in this file's subject: `ExtractText`'s entire help text sat
    800 lines away, welded onto `ListOutline`, so `list-outline --help`
    printed the text-extraction description and `extract-text --help` printed
    nothing. Found by eye, 2026-08-29.
  * `PrintPreview` and `RenderPage` were then found by an early version of
    THIS script, both shipping blank descriptions, neither related to any
    splice. That is the finding that justified writing it: **the class has
    more than one cause, so a rule about how to splice cannot close it.**

WHAT THIS CHECKS, AND WHAT IT DELIBERATELY DOES NOT
====================================================

Checks: every variant of `pdfcer`'s `Command` enum has a `///` doc comment
(or an explicit `#[command(about = ...)]`, which is the other way to supply
one).

Does NOT check: whether the doc comment is about the RIGHT command. That is
semantic and this script has no way to know. An earlier attempt at a
structural detector for the weld itself -- "a doc line whose predecessor is
non-empty and whose successor is blank" -- produced **8,136 candidates** over
`crates/`, because that is also the shape of every ordinary paragraph ending.
It was abandoned rather than shipped noisy, and the reasoning is recorded here
so the next session does not re-derive it.

So this gate catches the DONOR half of a weld (the item left with nothing),
not the RECIPIENT half (the item left with two). That is a real limitation and
it is stated rather than glossed: six of the eight instances left a donor,
which is why the coverage is worth having, and two did not.

EXIT CODES
==========

0 -- every variant carries help text.
1 -- one or more do not; each is named with its line number.
2 -- the script could not locate the enum (a refactor moved or renamed it),
     which is reported as a failure rather than a pass, because a gate that
     silently finds nothing to check is worse than no gate.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

MAIN = Path(__file__).resolve().parent.parent / "crates" / "pdfcer-cli" / "src" / "main.rs"

# A variant declaration at the enum's own indentation: `    Name {`, `    Name(`
# or `    Name,`. Anchored at exactly four spaces so a field inside a variant
# body (eight spaces) can never match.
VARIANT = re.compile(r"^    ([A-Z][A-Za-z0-9]*)\s*[{(,]")


def variants(lines: list[str], start: int) -> list[tuple[str, int, bool]]:
    """`(name, 1-based line, has_help)` for every variant of the enum at `start`.

    Brace-depth tracked explicitly rather than by regex over the whole file:
    variant bodies contain their own braces, and attributes such as
    `#[command(group = clap::ArgGroup::new("x"))]` contain parentheses and
    quotes that a line-oriented pattern mis-reads.

    Depth is evaluated BEFORE the current line is counted, so the line that
    OPENS a variant body is still seen at the enum's own depth. Getting that
    backwards is why a first draft of this script reported one variant in an
    enum of 111.
    """
    out: list[tuple[str, int, bool]] = []
    depth = 1
    i = start + 1
    while i < len(lines) and depth > 0:
        line = lines[i]
        m = VARIANT.match(line)
        if m and depth == 1:
            out.append((m.group(1), i + 1, has_help(lines, i)))
        depth += line.count("{") - line.count("}")
        i += 1
    return out


def has_help(lines: list[str], at: int) -> bool:
    """Whether the variant declared on line `at` carries operator help text.

    Walks backwards over attributes AND ordinary `//` comments. Both legitimately
    sit between a doc block and the item it documents: `UnembedFont` and
    `EmbedFont` each carry a multi-line `//` note about a `required(true)`
    hazard, placed after their `///` block and before their `#[command(...)]`
    attribute. An earlier draft walked back over `#[...]` only and reported both
    as undocumented -- two false positives out of four hits, which would have
    made the gate's first run half noise.
    """
    k = at - 1
    while k >= 0:
        s = lines[k].strip()
        if s.startswith("///"):
            return True
        # `#[command(about = "...")]` supplies the description directly.
        if s.startswith("#[") and "about" in s:
            return True
        if s.startswith("#[") or s.startswith("//"):
            k -= 1
            continue
        return False
    return False


def main() -> int:
    if not MAIN.is_file():
        print(f"check-clap-help: cannot read {MAIN}", file=sys.stderr)
        return 2
    lines = MAIN.read_text(encoding="utf-8").split("\n")
    starts = [i for i, l in enumerate(lines) if l.startswith("enum Command {")]
    if len(starts) != 1:
        print(
            "check-clap-help: expected exactly one `enum Command {` in "
            f"{MAIN.name}, found {len(starts)}. The enum was renamed or moved; "
            "this gate is not checking anything until that is fixed.",
            file=sys.stderr,
        )
        return 2

    found = variants(lines, starts[0])
    if not found:
        print(
            "check-clap-help: parsed the enum and found zero variants -- the "
            "declaration syntax changed and this gate is now blind.",
            file=sys.stderr,
        )
        return 2

    bare = [(n, ln) for n, ln, ok in found if not ok]
    if bare:
        print(
            f"check-clap-help: {len(bare)} of {len(found)} subcommand(s) ship "
            "with NO help text:",
            file=sys.stderr,
        )
        for name, ln in bare:
            print(f"  {MAIN.name}:{ln}  {name}", file=sys.stderr)
        print(
            "\nclap derives `--help` from the doc comment on the variant. Without\n"
            "one the subcommand appears in `pdfcer --help` with a blank\n"
            "description and its own `--help` opens with an empty line. Nothing\n"
            "else in the build notices -- not clippy, not missing_docs (these are\n"
            "private items in a binary crate), not any test, because no test reads\n"
            "help text.\n"
            "\n"
            "If the text exists but is attached elsewhere, it was probably ORPHANED\n"
            "by a splice that anchored on the variant name and landed inside the\n"
            "preceding variant's doc block. Look for a doc run with two summary\n"
            "lines and no blank `///` between them, then move the second half here.\n"
            "Insert AFTER a closing brace, never before a named anchor.",
            file=sys.stderr,
        )
        return 1

    print(f"check-clap-help: PASS — all {len(found)} subcommands carry help text.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
