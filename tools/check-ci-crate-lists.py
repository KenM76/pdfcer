"""Fail if a CI crate list omits a crate pdfcer-core re-exports.

`.github/workflows/ci.yml` (and any other workflow) names the engine crates by hand in the GUI-deps
tree, no-network tree and wasm32 steps. Splitting a crate out of pdfcer-core
means adding it to each list; a missed one silently drops that crate from the
check. Every list that names `pdfcer-model` must name every path dependency
pdfcer-core reaches (read from the Cargo.toml files).

usage: python tools/check-ci-crate-lists.py
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


DEP = re.compile(r'^(pdfcer-[\w-]+)\s*(?:=\s*\{[^}]*\b(?:path|workspace)\b|\.workspace\s*=\s*true)', re.M)


def runtime_deps(crate: str) -> set[str]:
    """`crate`'s pdfcer path dependencies, dev-dependencies excluded."""
    toml = (ROOT / "crates" / crate / "Cargo.toml").read_text(encoding="utf-8")
    found = set()
    for section in re.split(r"^\[", toml, flags=re.M):
        header = section.split("]", 1)[0]
        if header.endswith("dependencies") and "dev-dependencies" not in header:
            found |= set(DEP.findall(section))
    return found


def lower_crates() -> set[str]:
    """Every pdfcer crate pdfcer-core reaches, transitively: a crate pulled in
    through another (pdfcer-jpeg-remux through pdfcer-image-codec) is in the
    engine graph as much as a direct one."""
    seen, todo = set(), ["pdfcer-core"]
    while todo:
        for dep in runtime_deps(todo.pop()) - seen:
            seen.add(dep)
            todo.append(dep)
    return seen


MODEL = re.compile(r"(?<![\w-])pdfcer-model(?![\w-])")
PACKAGE = re.compile(r"\bcargo\s+package\b")


def is_list(line: str) -> bool:
    """A command line naming pdfcer-model. Comments are prose, and a
    `cargo package` list names the crates being made publishable, a
    deliberate subset guarding no invariant over the engine."""
    return (
        bool(MODEL.search(line))
        and not line.lstrip().startswith("#")
        and not PACKAGE.search(line)
    )


def scan(wf: Path, need: set[str]) -> list[str]:
    bad = []
    for n, line in enumerate(wf.read_text(encoding="utf-8").splitlines(), 1):
        if not is_list(line):
            continue
        # A name may end in a digit (`pdfcer-vec3`); trailing `-` or `/` is punctuation.
        named = set(re.findall(r"(?<![\w-])(pdfcer-[a-z0-9-]*[a-z0-9])", line))
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
        lists += sum(1 for line in wf.read_text(encoding="utf-8").splitlines() if is_list(line))
    for b in bad:
        print("FAIL", b)
    if bad:
        return 1
    print(f"PASS ci crate lists ({lists} lists x {len(need)} crates)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
