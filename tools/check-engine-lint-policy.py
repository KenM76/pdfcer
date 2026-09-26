"""Fail if an engine crate's lib.rs drops the panic-free or no-unsafe lints.

Engine crates parse untrusted input (ARCHITECTURE.md §10), so each denies
`unwrap_used`, `expect_used`, `panic` and `indexing_slicing` crate-wide and
forbids `unsafe_code`. Splitting a module out of pdfcer-core into a new crate
starts a fresh lib.rs, and nothing else notices the policy did not follow it.

Exceptions, each for a stated reason:
- pdfcer-print: calls the platform print API, so it cannot forbid unsafe.
- pdfcer-cli, pdfcer-fetch: shells, not parsers of untrusted PDF bytes.

usage: python tools/check-engine-lint-policy.py
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DENY = ("unwrap_used", "expect_used", "panic", "indexing_slicing")
NO_DENY = {"pdfcer-cli", "pdfcer-fetch"}
NO_FORBID = {"pdfcer-print", "pdfcer-cli"}
WAIVED: dict[str, set[str]] = {}


def main() -> int:
    bad = []
    crates = sorted(p.parent for p in (ROOT / "crates").glob("*/Cargo.toml"))
    for crate in crates:
        lib = crate / "src" / "lib.rs"
        if not lib.exists():
            continue
        src = lib.read_text(encoding="utf-8")
        name = crate.name
        if name not in NO_DENY:
            block = " ".join(re.findall(r"#!\[deny\(([^\]]*)\)\]", src))
            missing = [d for d in DENY if f"clippy::{d}" not in block]
            waived = WAIVED.get(name, set())
            stale = [d for d in waived if f"clippy::{d}" in block]
            if stale:
                bad.append(f"{name}: now denies {', '.join(stale)}; drop its WAIVED entry")
            missing = [d for d in missing if d not in waived]
            if missing:
                bad.append(f"{name}: #![deny] lacks {', '.join(missing)}")
        if name not in NO_FORBID and "#![forbid(unsafe_code)]" not in src:
            bad.append(f"{name}: lacks #![forbid(unsafe_code)]")
    for b in bad:
        print("FAIL", b)
    if bad:
        return 1
    print(f"PASS engine lint policy ({len(crates)} crates)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
