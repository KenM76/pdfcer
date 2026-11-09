#!/usr/bin/env python3
"""Gate: `pdfcer-render --no-default-features` pulls in no optional capability.

A crates.io consumer takes `pdfcer-render` with `default-features = false`
to get rendering, SVG export and rich text without OCR, signing, 3D or SVG
import (request X061). This checks that the normal-dependency graph of that
shape contains none of the crates pdfcer-core makes optional behind a
default feature. The banned set is read from the `[features]` tables of
pdfcer-core and pdfcer-render (every `dep:` name), so a new optional
capability in either is covered without editing this file.

Exit 0 = clean, 1 = an optional crate leaked into the lean graph, 2 = could
not run cargo or parse the manifest.
"""

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MANIFESTS = [ROOT / "crates" / name / "Cargo.toml" for name in ("pdfcer-core", "pdfcer-render")]


def optional_crates() -> set[str]:
    banned: set[str] = set()
    for manifest in MANIFESTS:
        text = manifest.read_text(encoding="utf-8")
        m = re.search(r"(?ms)^\[features\]\s*$(.*?)^\[", text)
        if not m:
            print(f"check-lean-render-graph: no [features] table in {manifest}", file=sys.stderr)
            sys.exit(2)
        banned |= set(re.findall(r'"dep:([A-Za-z0-9_-]+)"', m.group(1)))
    return banned


def lean_graph() -> set[str]:
    cmd = ["cargo", "tree", "-p", "pdfcer-render", "--no-default-features",
           "-e", "normal", "--prefix", "none"]
    try:
        out = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True, check=True).stdout
    except (OSError, subprocess.CalledProcessError) as e:
        print(f"check-lean-render-graph: {' '.join(cmd)} failed: {e}", file=sys.stderr)
        sys.exit(2)
    return {line.split(" ")[0] for line in out.splitlines() if line.strip()}


def main() -> int:
    banned = optional_crates()
    if not banned:
        print("check-lean-render-graph: found no optional deps in pdfcer-core or pdfcer-render", file=sys.stderr)
        return 2
    graph = lean_graph()
    leaked = sorted(banned & graph)
    if leaked:
        print("pdfcer-render --no-default-features pulls in optional capability crates:")
        for name in leaked:
            print(f"  {name}")
        print("A feature forwarding or a non-optional dependency let them in; a lean "
              "consumer (X061) would compile them.")
        return 1
    print(f"check-lean-render-graph: ok ({len(graph)} crates, none of {len(banned)} optional)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
