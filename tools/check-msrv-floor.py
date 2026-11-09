#!/usr/bin/env python3
"""Gate: the workspace `rust-version` is not below any dependency's.

The declared MSRV is the highest `rust-version` in the resolved dependency
graph (decision 210). A dependency upgrade or a vendor sync that raises its
own floor makes the declaration false without any build failing on the
pinned toolchain; this catches it.

Exit 0 = clean, 1 = a package needs a newer compiler than declared,
2 = could not run cargo or read the workspace value.
"""

import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def version(text: str) -> tuple[int, ...]:
    return tuple(int(p) for p in re.findall(r"\d+", text)[:3])


def workspace_floor() -> str:
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    m = re.search(r'(?m)^rust-version\s*=\s*"([^"]+)"', text)
    if not m:
        print("check-msrv-floor: no rust-version in the root Cargo.toml", file=sys.stderr)
        sys.exit(2)
    return m.group(1)


def main() -> int:
    declared = workspace_floor()
    cmd = ["cargo", "metadata", "--format-version", "1", "--locked"]
    try:
        out = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True,
                             encoding="utf-8", check=True).stdout
    except (OSError, subprocess.CalledProcessError) as e:
        print(f"check-msrv-floor: {' '.join(cmd)} failed: {e}", file=sys.stderr)
        return 2
    meta = json.loads(out)
    members = set(meta["workspace_members"])  # they inherit the declared value
    above = sorted(
        (p["name"], p["version"], p["rust_version"])
        for p in meta["packages"]
        if p["id"] not in members
        and p.get("rust_version")
        and version(p["rust_version"]) > version(declared)
    )
    if above:
        print(f"workspace rust-version {declared} is below these dependencies' floors:")
        for name, ver, rv in above:
            print(f"  {name} {ver} needs {rv}")
        print("Raise rust-version in the root Cargo.toml, with a decision-log entry.")
        return 1
    print(f"check-msrv-floor: ok (rust-version {declared})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
