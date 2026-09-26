"""Fail if a CI crate list omits a crate pdfcer-core re-exports.

`.github/workflows/ci.yml` names the engine crates by hand in the GUI-deps
tree, no-network tree and wasm32 steps. Splitting a crate out of pdfcer-core
means adding it to each list; a missed one silently drops that crate from the
check. Every list that names `pdfcer-model` must name every path dependency
of pdfcer-core (read from its Cargo.toml).

usage: python tools/check-ci-crate-lists.py
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def lower_crates() -> set[str]:
    toml = (ROOT / "crates" / "pdfcer-core" / "Cargo.toml").read_text(encoding="utf-8")
    return set(re.findall(r'^(pdfcer-[\w-]+)\s*=\s*\{\s*path\s*=', toml, re.M))


def main() -> int:
    need = lower_crates()
    if "pdfcer-model" not in need:
        print("FAIL could not read pdfcer-core's path dependencies")
        return 1
    ci = (ROOT / ".github" / "workflows" / "ci.yml").read_text(encoding="utf-8")
    bad, lists = [], 0
    for n, line in enumerate(ci.splitlines(), 1):
        if not re.search(r"(?<![\w-])pdfcer-model(?![\w-])", line):
            continue
        lists += 1
        named = set(re.findall(r"(?<![\w-])(pdfcer-[a-z-]+[a-z])", line))
        missing = sorted(need - named)
        if missing:
            bad.append(f"ci.yml:{n} lacks {', '.join(missing)}")
    for b in bad:
        print("FAIL", b)
    if bad:
        return 1
    print(f"PASS ci crate lists ({lists} lists x {len(need)} crates)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
