#!/usr/bin/env python3
"""check-core-api-no-line-citations.py -- docs/core-api cites symbols, not lines.

A line number into a source file decays with every edit to that file, and no
tool re-derives one. When measured, 208 of the ~1,365 citations pointed past
end-of-file and 250 more pointed at the wrong item; only 142 could be shown
correct. The remedy (ROADMAP Pass 259.0, remedy (b)) cites the file and the
symbol and lets the reader grep.

Fails on, in any ``docs/core-api/*.md``:
  * a code span ``file.rs:N`` / ``file.md:N`` / ``file.toml:N`` (ranges too);
  * a bare code span ``:N``;
  * a table whose header has a ``Line`` column.
Commit-pinned citations (``repo@hash:path:N``) never decay and are allowed.

Exit 0 clean, 1 on any finding (each printed as ``path:line: text``).
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
DOCS = sorted((ROOT / "docs" / "core-api").glob("*.md"))

TICK = chr(96)
SPAN = re.compile(TICK + "([^" + TICK + "\n]+)" + TICK)
NUM = r":\d+(?:[-–]\d+)?(?:,\s*\d+(?:-\d+)?)*\+?"
CITE = re.compile(r"^(?:[A-Za-z0-9_./-]+\.(?:rs|md|toml))?~?" + NUM + "$")
LINE_HEADER = re.compile(r"^\s*[Ll]ines?\s*$")
SEPARATOR = re.compile(r"^\s*\|[\s:|-]+\|\s*$")


def cells(row):
    return [c.strip() for c in row.strip().strip("|").split("|")]


def main():
    bad = []
    for doc in DOCS:
        lines = doc.read_text(encoding="utf-8").split("\n")
        rel = doc.relative_to(ROOT).as_posix()
        for n, text in enumerate(lines, 1):
            for m in SPAN.finditer(text):
                span = m.group(1).strip()
                if "@" not in span and CITE.match(span):
                    bad.append(f"{rel}:{n}: line citation `{span}`")
            if (text.lstrip().startswith("|") and n < len(lines)
                    and SEPARATOR.match(lines[n])
                    and any(LINE_HEADER.match(c) for c in cells(text))):
                bad.append(f"{rel}:{n}: table has a Line column")
    for b in bad:
        print(b)
    if bad:
        print(f"{len(bad)} line citation(s) in docs/core-api: cite the file "
              "and symbol instead (see docs/core-api/index.md).")
        return 1
    print(f"core-api line citations: none in {len(DOCS)} files")
    return 0


if __name__ == "__main__":
    sys.exit(main())
