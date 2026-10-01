#!/usr/bin/env python3
"""Structure gate: long functions, long files, copy-pasted helpers.

Measures PRODUCTION Rust code only (everything before a file's first
`#[cfg(test)]`; files under `tests/`, `benches/`, `fuzz/` and `examples/`
are skipped):

* a function longer than MAX_FN_LINES lines;
* a file whose production part is longer than MAX_FILE_LINES lines;
* a private helper defined with the same name AND the same body (whitespace
  ignored) in two or more files of one crate -- the copy-paste signature.

Existing violations live in tools/code-structure-baseline.txt. The baseline
is DEBT, not an allowlist: a NEW violation fails, a baseline entry that no
longer occurs fails too (delete the line), so the list only ever shrinks.

Usage: check-code-structure.py [--root DIR] [--write-baseline]
Exit 0 clean, 1 on any new or stale entry.
"""

import argparse
import re
import sys
from collections import defaultdict
from pathlib import Path

MAX_FN_LINES = 80
MAX_FILE_LINES = 800
SKIP_DIRS = {"tests", "benches", "fuzz", "examples", "target", ".git", ".claude"}
FN_RE = re.compile(r"^\s*(?:pub(?:\([a-z]+\))?\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+(\w+)")


def production_lines(text: str) -> list[str]:
    lines = text.split("\n")
    for i, line in enumerate(lines):
        if line.strip().startswith("#[cfg(test)]"):
            return lines[:i]
    return lines


def functions(lines: list[str]):
    """Yield (name, first_line_index, length, body_text) per top-level-ish fn."""
    i = 0
    while i < len(lines):
        m = FN_RE.match(lines[i])
        if not m:
            i += 1
            continue
        depth, started, j = 0, False, i
        while j < len(lines):
            code = lines[j].split("//", 1)[0]
            if not started and ";" in code and "{" not in code:
                break  # trait method declaration, no body
            depth += code.count("{") - code.count("}")
            if "{" in code:
                started = True
            if started and depth <= 0:
                break
            j += 1
        if started:
            body = "".join("".join(l.split()) for l in lines[i + 1 : j + 1])
            yield m.group(1), i, j - i + 1, body
            i = j + 1
        else:
            i = j + 1


def crate_of(path: Path, root: Path) -> str:
    for parent in path.parents:
        if (parent / "Cargo.toml").exists():
            return parent.relative_to(root).as_posix() or "."
    return "."


def measure(root: Path) -> set[str]:
    found: set[str] = set()
    helpers: dict[tuple[str, str, str], set[str]] = defaultdict(set)
    for path in sorted(root.rglob("*.rs")):
        rel = path.relative_to(root)
        if any(part in SKIP_DIRS for part in rel.parts):
            continue
        lines = production_lines(path.read_text(encoding="utf-8", errors="replace"))
        name = rel.as_posix()
        if len(lines) > MAX_FILE_LINES:
            found.add(f"file {name}")
        for fn, start, length, body in functions(lines):
            if length > MAX_FN_LINES:
                found.add(f"fn {name}::{fn}")
            if lines[start].startswith("fn "):  # private free fn; impl methods may legitimately repeat
                helpers[(crate_of(path, root), fn, body)].add(name)
    for (crate, fn, _), files in helpers.items():
        if len(files) > 1:
            found.add(f"dup {crate}::{fn} in {', '.join(sorted(files))}")
    return found


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--root", default=str(Path(__file__).resolve().parent.parent))
    ap.add_argument("--write-baseline", action="store_true")
    args = ap.parse_args()
    root = Path(args.root)
    baseline_path = Path(__file__).resolve().parent / "code-structure-baseline.txt"
    found = measure(root / "crates") if (root / "crates").is_dir() else measure(root)
    if args.write_baseline:
        baseline_path.write_bytes(("\n".join(sorted(found)) + "\n").encode())
        print(f"code-structure: baseline written, {len(found)} entr(ies)")
        return 0
    baseline = {
        l.strip()
        for l in baseline_path.read_text(encoding="utf-8").splitlines()
        if l.strip() and not l.startswith("#")
    } if baseline_path.exists() else set()
    new, stale = sorted(found - baseline), sorted(baseline - found)
    for entry in new:
        print(f"NEW   {entry}")
    for entry in stale:
        print(f"STALE {entry}  (fixed -- delete this baseline line)")
    if new or stale:
        print(f"code-structure: FAIL -- {len(new)} new, {len(stale)} stale; "
              f"limits fn {MAX_FN_LINES} lines, file {MAX_FILE_LINES} production lines, "
              "no duplicated private helpers")
        return 1
    print(f"code-structure: clean -- {len(found)} baseline entr(ies) of debt, none new")
    return 0


if __name__ == "__main__":
    sys.exit(main())
