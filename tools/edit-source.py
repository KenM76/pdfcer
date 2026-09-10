#!/usr/bin/env python3
"""Line-ending-agnostic exact replacement for source edits.

# Why this exists

A multi-line `str.replace` against one of this project's files matches **zero
times** when the pattern is typed with `\\n` and the file is CRLF — and
`str.replace` reports that by **doing nothing at all**. A silent no-op is
indistinguishable from success, so the edit appears to have been applied and
the next command fails somewhere unrelated.

That happened **three separate times in one session** (2026-09-10), each time
costing a diagnostic detour. `docs/NEXT_SESSION.md` had already carried a
warning about it; a warning is not a fix, which is the whole argument for this
file existing.

# What it guarantees

* patterns are normalised to **the file's own dominant line ending** before
  matching, and replacements are emitted in it;
* every replacement must match **exactly once** — zero matches and two matches
  are both refusals, because "which of the two did you mean?" has no safe
  default;
* **nothing is written unless every replacement succeeds**, so a failed run
  leaves the file exactly as it found it rather than half-edited.

# Usage

    python tools/edit-source.py <file> <old1> <new1> [<old2> <new2> ...]

Each `<old>`/`<new>` is a **path to a file** holding the literal text, not the
text itself — which is the point. Passing patterns as shell arguments is how
backticks get command-substituted and how backslashes get eaten; this project
has lost content in a pushed commit message to exactly that. Write the payload
with a file-writing tool and pass its path.

Exit codes: `0` applied, `1` a pattern did not match exactly once, `2` usage.
"""

import io
import sys


def dominant_newline(text: str) -> str:
    """The line ending the file mostly uses.

    Counted rather than sniffed from the first line: this project has files
    with mixed endings (a CRLF file spliced with LF blocks), and the majority
    is what a pattern should be normalised to.
    """
    crlf = text.count("\r\n")
    bare_lf = text.count("\n") - crlf
    return "\r\n" if crlf >= bare_lf else "\n"


def normalise(pattern: str, newline: str) -> str:
    """Re-express `pattern` in `newline`, whatever it arrived in."""
    return pattern.replace("\r\n", "\n").replace("\n", newline)


def read_pattern(path: str) -> str:
    """Read a pattern file, dropping the trailing newline a text editor adds.

    Without this, every pattern would have to end exactly where its file does,
    and a file that ends with a newline (all of them) could never match a
    pattern that does not.
    """
    return io.open(path, encoding="utf-8", newline="").read().rstrip("\r\n")


def main(argv: list[str]) -> int:
    if len(argv) < 4 or len(argv) % 2 != 0:
        print(__doc__, file=sys.stderr)
        return 2

    path = argv[1]
    pairs = argv[2:]

    source = io.open(path, encoding="utf-8", newline="").read()
    newline = dominant_newline(source)
    edited = source

    for i in range(0, len(pairs), 2):
        old = normalise(read_pattern(pairs[i]), newline)
        new = normalise(read_pattern(pairs[i + 1]), newline)
        found = edited.count(old)
        if found != 1:
            print(
                f"edit-source: {pairs[i]} matched {found} time(s), need exactly 1."
                f"\n  file: {path}"
                f"\n  (nothing was written)",
                file=sys.stderr,
            )
            return 1
        edited = edited.replace(old, new, 1)

    io.open(path, "w", encoding="utf-8", newline="").write(edited)
    print(f"edit-source: {len(pairs) // 2} replacement(s) applied to {path}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
