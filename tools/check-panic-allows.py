#!/usr/bin/env python3
"""Panic-free gate for the engine crates.

check-engine-lint-policy.py makes each engine crate deny PANIC_LINTS. A
crate-level deny is only as strong as its exemptions, so every
`#[allow(...)]` / `#[expect(...)]` naming one of those lints in PRODUCTION
code (outside test modules, `tests.rs` / `*_tests.rs` files and `tests/`
directories) is listed in tools/panic-allows-baseline.txt.

The baseline is DEBT, not an allowlist: a NEW exemption fails, and so does a
baseline entry that no longer occurs (delete the line), so the list only ever
shrinks. A kept exemption must say on or above its line why the call cannot
panic.

Entries are keyed `path | lints | next item` so they survive line moves.

Usage: check-panic-allows.py [--root DIR] [--write-baseline]
Exit 0 clean, 1 on a new or stale entry.
"""

import argparse
import re
import sys
from collections import Counter
from pathlib import Path

PANIC_LINTS = (
    "unwrap_used",
    "expect_used",
    "panic",
    "indexing_slicing",
    "unreachable",
    "todo",
    "unimplemented",
)
LINT_RE = re.compile(r"clippy::(" + "|".join(PANIC_LINTS) + r")\b")
ATTR_RE = re.compile(r"^\s*#!?\[(?:allow|expect)\(")
TEST_ATTR_RE = re.compile(r"^\s*#\[cfg\((?:all\()?test\b")
ITEM_RE = re.compile(
    r"^\s*(?:pub(?:\([a-z]+\))?\s+)?(?:const\s+|async\s+|unsafe\s+)*"
    r"(fn|impl|struct|enum|mod|trait|static|const|let)\b\s*([^\s(<{:=]*)"
)
BASELINE = Path("tools/panic-allows-baseline.txt")


def is_test_file(path: Path) -> bool:
    return (
        path.name == "tests.rs"
        or path.name.endswith("_tests.rs")
        or "tests" in path.parts[:-1]
    )


def item_end(lines: list[str], i: int) -> int:
    """The line after the item starting at line i (brace-matched, or `;`)."""
    depth, started = 0, False
    for j in range(i, len(lines)):
        code = lines[j].split("//", 1)[0]
        if not started and ";" in code and "{" not in code:
            return j + 1
        depth += code.count("{") - code.count("}")
        started = started or "{" in code
        if started and depth <= 0:
            return j + 1
    return len(lines)


def production_lines(text: str) -> list[str]:
    """`text` with every `#[cfg(test)]`-gated item blanked out."""
    lines = text.split("\n")
    out, i = [], 0
    while i < len(lines):
        if not TEST_ATTR_RE.match(lines[i]):
            out.append(lines[i])
            i += 1
            continue
        j = i + 1
        while j < len(lines) and (
            not lines[j].strip() or lines[j].lstrip().startswith(("#[", "//"))
        ):
            j = attribute_text(lines, j)[1] if lines[j].lstrip().startswith("#[") else j + 1
        end = item_end(lines, j)
        out.extend([""] * (end - i))
        i = end
    return out


def next_item(lines: list[str], start: int) -> str:
    for line in lines[start:]:
        m = ITEM_RE.match(line)
        if m:
            return f"{m.group(1)} {m.group(2)}".strip()
    return "?"


def attribute_text(lines: list[str], i: int) -> tuple[str, int]:
    """The attribute starting at line i, joined, and the line after it."""
    text, j = lines[i], i
    while text.count("(") > text.count(")") and j + 1 < len(lines):
        j += 1
        text += lines[j]
    return text, j + 1


def gated_crates(root: Path) -> list[Path]:
    out = []
    for lib in sorted(root.glob("crates/*/src/lib.rs")):
        m = re.search(r"#!\[deny\((.*?)\)\]", lib.read_text(encoding="utf-8"), re.S)
        if m and "clippy::unwrap_used" in m.group(1):
            out.append(lib.parent.parent)
    return out


def exemptions(root: Path, crate: Path) -> Counter:
    found = Counter()
    for path in sorted((crate / "src").rglob("*.rs")):
        rel = path.relative_to(root)
        if is_test_file(rel):
            continue
        lines = production_lines(path.read_text(encoding="utf-8"))
        i = 0
        while i < len(lines):
            if lines[i].lstrip().startswith("#![deny") or not ATTR_RE.match(lines[i]):
                i += 1
                continue
            text, after = attribute_text(lines, i)
            lints = sorted(set(LINT_RE.findall(text.split("//", 1)[0])))
            if lints:
                key = f"{rel.as_posix()} | {','.join(lints)} | {next_item(lines, after)}"
                found[key] += 1
            i = after
    return found


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--root", type=Path, default=Path("."))
    ap.add_argument("--write-baseline", action="store_true")
    args = ap.parse_args()
    root = args.root

    crates = gated_crates(root)
    current = Counter()
    for crate in crates:
        current += exemptions(root, crate)

    lines = sorted(f"{k}" for k, n in current.items() for _ in range(n))
    if args.write_baseline:
        (root / BASELINE).write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
        print(f"panic-allows: wrote {len(lines)} baseline entr(ies)")
        return 0

    base_path = root / BASELINE
    base = Counter(
        l for l in base_path.read_text(encoding="utf-8").splitlines() if l.strip()
    ) if base_path.exists() else Counter()
    new = current - base
    stale = base - current

    for key in sorted(new.elements()):
        print(f"NEW      {key}")
    for key in sorted(stale.elements()):
        print(f"STALE    {key}  (fixed -- delete the line from {BASELINE.as_posix()})")
    if new or stale:
        print(
            "panic-allows: FAIL -- a production exemption from a panic lint needs "
            "removing, not baselining; a fixed one must leave the baseline"
        )
        return 1
    print(
        f"panic-allows: clean -- {len(crates)} crate(s); "
        f"{sum(base.values())} baseline exemption(s) of debt, none new"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
