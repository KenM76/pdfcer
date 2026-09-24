#!/usr/bin/env python3
"""check-ocrcer-vendored.py -- the vendored OCRcer matches OCRcer's local HEAD.

The operator's rule (decision 160): pdfcer always builds the newest LOCAL
OCRcer, not its possibly-older GitHub mirror. ``tools/sync-ocrcer.py`` does
the copy; this gate makes a forgotten sync fail the sweep instead of shipping
an old recogniser silently.

Checks, byte for byte against OCRcer ``HEAD``: every file under
``vendor/ocrcer-core`` (manifest as rewritten by the sync), no extra files,
and ``crates/pdfcer-core/src/ocr/engine_ocrcer.rs`` against OCRcer's adapter.

With no OCRcer checkout beside the repository (GitHub CI, any other clone)
there is nothing newer to compare against; it reports that and passes.

Exit: 0 current or no checkout, 1 stale (run ``python tools/sync-ocrcer.py``).
"""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("sync_ocrcer", HERE / "sync-ocrcer.py")
sync = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(sync)


def main() -> int:
    source = sync.DEFAULT_SOURCE
    if not (source / ".git").exists():
        print(f"check-ocrcer-vendored: no OCRcer checkout at {source}; nothing to compare, OK")
        return 0
    rev = sync.head(source)
    want = sync.expected_files(source, rev)
    have = {
        p.relative_to(sync.VENDOR).as_posix(): p.read_bytes()
        for p in sync.VENDOR.rglob("*")
        if p.is_file()
    } if sync.VENDOR.exists() else {}
    problems = []
    for rel in sorted(set(want) | set(have)):
        if rel not in have:
            problems.append(f"missing  vendor/ocrcer-core/{rel}")
        elif rel not in want:
            problems.append(f"extra    vendor/ocrcer-core/{rel}")
        elif want[rel] != have[rel]:
            problems.append(f"differs  vendor/ocrcer-core/{rel}")
    adapter = sync.show(source, rev, sync.ADAPTER_SRC)
    if not sync.ADAPTER_DEST.is_file() or sync.ADAPTER_DEST.read_bytes() != adapter:
        problems.append("differs  crates/pdfcer-core/src/ocr/engine_ocrcer.rs")
    if problems:
        print(f"check-ocrcer-vendored: FAILED -- vendored OCRcer is not OCRcer HEAD {rev[:12]}:")
        for p in problems:
            print(f"  {p}")
        print("  fix: python tools/sync-ocrcer.py")
        return 1
    print(f"check-ocrcer-vendored: OK -- vendored OCRcer is HEAD {rev[:12]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
