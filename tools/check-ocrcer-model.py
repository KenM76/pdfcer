#!/usr/bin/env python3
"""Verify an `ocrcer.ocrw` model file against the model pdfcer vendored with.

    python tools/check-ocrcer-model.py <path/to/ocrcer.ocrw>

Compares the file's size and sha256 with `model-bytes` / `model-sha256` in
`vendor/ocrcer-core/VENDORED` (copied from OCRcer's `model/MODEL.toml` by
`tools/sync-ocrcer.py`). A packager runs this on the model it copies into a
portable folder, so a shipped model is provably the one this reader was
synced alongside.

Exit 0 on a match, 1 on a mismatch or unreadable input, printing why.
"""

from __future__ import annotations

import hashlib
import re
import sys
from pathlib import Path

VENDORED = Path(__file__).resolve().parent.parent / "vendor" / "ocrcer-core" / "VENDORED"


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__.strip().split("\n\n")[1], file=sys.stderr)
        return 1
    record = dict(re.findall(r"^([\w-]+) = (\S+)$", VENDORED.read_text(), re.M))
    if "model-sha256" not in record:
        print("check-ocrcer-model: VENDORED carries no model-sha256; re-run sync-ocrcer.py")
        return 1
    model = Path(sys.argv[1])
    try:
        data = model.read_bytes()
    except OSError as e:
        print(f"check-ocrcer-model: cannot read {model}: {e}")
        return 1
    got = hashlib.sha256(data).hexdigest()
    want = record["model-sha256"]
    release = record.get("model-release", "?")
    if len(data) != int(record["model-bytes"]) or got != want:
        print(
            f"check-ocrcer-model: MISMATCH {model}\n"
            f"  have {len(data)} bytes, sha256 {got}\n"
            f"  want {record['model-bytes']} bytes, sha256 {want} (OCRcer {release})"
        )
        return 1
    print(f"check-ocrcer-model: OK {model} is OCRcer {release} ({want[:12]})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
