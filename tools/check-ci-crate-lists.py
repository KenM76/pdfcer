"""Fail if a CI crate list omits a crate pdfcer-core re-exports.

`.github/workflows/ci.yml` (and any other workflow) names the engine crates by hand in the GUI-deps
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


MODEL = re.compile(r"(?<![\w-])pdfcer-model(?![\w-])")


def scan(wf: Path, need: set[str]) -> list[str]:
    bad = []
    for n, line in enumerate(wf.read_text(encoding="utf-8").splitlines(), 1):
        if not MODEL.search(line):
            continue
        named = set(re.findall(r"(?<![\w-])(pdfcer-[a-z-]+[a-z])", line))
        missing = sorted(need - named)
        if missing:
            bad.append(f"{wf.name}:{n} lacks {', '.join(missing)}")
    return bad


def main() -> int:
    need = lower_crates()
    if "pdfcer-model" not in need:
        print("FAIL could not read pdfcer-core's path dependencies")
        return 1
    bad, lists = [], 0
    wfdir = ROOT / ".github" / "workflows"
    # Every workflow, both extensions: a list in a second file is as live as one in ci.yml.
    for wf in sorted([*wfdir.glob("*.yml"), *wfdir.glob("*.yaml")]):
        bad += scan(wf, need)
        lists += sum(1 for line in wf.read_text(encoding="utf-8").splitlines() if MODEL.search(line))
    for b in bad:
        print("FAIL", b)
    if bad:
        return 1
    print(f"PASS ci crate lists ({lists} lists x {len(need)} crates)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
