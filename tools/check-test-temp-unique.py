#!/usr/bin/env python3
"""Fail if a test names a fixed path under the system temp directory.

A fixed name (`temp_dir().join("pdfcer-x-tests")`) is shared by every test
process on the machine, so two concurrent runs (a gate sweep beside a
worktree agent's run, or two CI jobs on one runner) delete or overwrite each
other's files mid-test. The failure reads as "file not found" in an unrelated
test. Include `std::process::id()` (or another per-process component) in the
name.

Scans `crates/*/tests/**/*.rs`. Exit 0 clean, 1 on a finding.
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
FIXED = re.compile(r'temp_dir\(\)\s*\.join\(\s*"[^"{]*"\s*\)')


def main() -> int:
    hits = []
    for path in sorted(ROOT.glob("crates/*/tests/**/*.rs")):
        text = path.read_text(encoding="utf-8", errors="replace")
        for m in FIXED.finditer(text):
            line = text.count("\n", 0, m.start()) + 1
            hits.append(f"{path.relative_to(ROOT).as_posix()}:{line}: {m.group(0)}")
    if hits:
        print("test-temp-unique: FAILED -- a fixed temp path is shared by concurrent test runs:")
        for h in hits:
            print("  " + h)
        print("Add std::process::id() to the name.")
        return 1
    print("test-temp-unique: clean -- every temp path in tests is per-process")
    return 0


if __name__ == "__main__":
    sys.exit(main())
