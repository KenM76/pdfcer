#!/usr/bin/env python3
"""sync-ocrcer.py -- vendor the newest local OCRcer into this repository.

OCRcer (``D:\\Dev\\OCRcer``, MIT, same operator) is developed locally ahead
of its GitHub mirror, and the operator's standing rule is that pdfcer always
builds the newest LOCAL version (decision 160). A path dependency on
``../OCRcer`` cannot be committed: Cargo reads every path dependency's
manifest to resolve the workspace, so any clone without the sibling checkout
-- GitHub CI included -- would not build at all.

So the newest local commit is copied in:

* ``crates/ocrcer-core``  (OCRcer ``HEAD``)  -> ``vendor/ocrcer-core``, its
  manifest rewritten to concrete ``edition``/``license``/``rust-version``
  (OCRcer's are ``workspace = true``, which cannot resolve here);
* ``integration/pdfcer/ocrcer_engine.rs`` -> ``crates/pdfcer-core/src/ocr/engine_ocrcer.rs``,
  unmodified -- the adapter is OCRcer's to write;
* ``LICENSE`` -> ``vendor/ocrcer-core/LICENSE``;
* ``vendor/ocrcer-core/VENDORED`` records the source commit.

It copies COMMITTED content (``git show HEAD:<path>``), never the working
tree: OCRcer's own session may be mid-edit, and a half-written file must not
reach a pdfcer build. ``tools/check-ocrcer-vendored.py`` fails the gate sweep
when the vendored copy is behind OCRcer's ``HEAD``.

Usage:  python tools/sync-ocrcer.py [--source D:/Dev/OCRcer]
Exit:   0 synced (or already current), 1 source missing or unreadable.
"""

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
DEFAULT_SOURCE = REPO.parent / "OCRcer"
VENDOR = REPO / "vendor" / "ocrcer-core"
ADAPTER_DEST = REPO / "crates" / "pdfcer-core" / "src" / "ocr" / "engine_ocrcer.rs"
ADAPTER_SRC = "integration/pdfcer/ocrcer_engine.rs"
CRATE_SRC = "crates/ocrcer-core"


def git(source: Path, *args: str) -> bytes:
    return subprocess.run(
        ["git", "-C", str(source), *args], check=True, capture_output=True
    ).stdout


def head(source: Path) -> str:
    return git(source, "rev-parse", "HEAD").decode().strip()


def show(source: Path, rev: str, path: str) -> bytes:
    return git(source, "show", f"{rev}:{path}")


def workspace_package(source: Path, rev: str) -> dict[str, str]:
    text = show(source, rev, "Cargo.toml").decode()
    block = re.search(r"^\[workspace\.package\](.*?)(?=^\[|\Z)", text, re.S | re.M)
    if not block:
        raise SystemExit("sync-ocrcer: OCRcer Cargo.toml has no [workspace.package]")
    return dict(re.findall(r'^(\w[\w-]*)\s*=\s*"([^"]*)"', block.group(1), re.M))


def rewrite_manifest(manifest: str, ws: dict[str, str]) -> str:
    def sub(m: re.Match[str]) -> str:
        key = m.group(1)
        if key not in ws:
            raise SystemExit(f"sync-ocrcer: {key}.workspace has no workspace value")
        return f'{key} = "{ws[key]}"'

    out = re.sub(r"^([\w-]+)\.workspace[ \t]*=[ \t]*true[ \t]*$", sub, manifest, flags=re.M)
    if "workspace = true" in out:
        raise SystemExit("sync-ocrcer: an inherited table remains in the manifest")
    header = (
        "# VENDORED from OCRcer by tools/sync-ocrcer.py -- do not edit here;\n"
        "# edit in OCRcer and re-sync. Source commit: vendor/ocrcer-core/VENDORED.\n"
    )
    return header + out


def expected_files(source: Path, rev: str) -> dict[str, bytes]:
    """Relative path under vendor/ocrcer-core -> bytes, for the given rev."""
    names = git(source, "ls-tree", "-r", "--name-only", rev, CRATE_SRC).decode().split()
    ws = workspace_package(source, rev)
    files: dict[str, bytes] = {}
    for name in names:
        rel = name[len(CRATE_SRC) + 1 :]
        data = show(source, rev, name)
        if rel == "Cargo.toml":
            data = rewrite_manifest(data.decode(), ws).encode()
        files[rel] = data
    files["LICENSE"] = show(source, rev, "LICENSE")
    # The last commit that changed anything vendored -- not HEAD, which moves
    # on every OCRcer doc commit and would make the gate fail on no change.
    last = git(
        source, "log", "-1", "--format=%H", rev, "--",
        CRATE_SRC, "LICENSE", "Cargo.toml", ADAPTER_SRC,
    ).decode().strip()
    files["VENDORED"] = (
        f"source = https://github.com/KenM76/ocrcer (local checkout)\n"
        f"commit = {last}\n"
        f"synced-by = tools/sync-ocrcer.py\n"
    ).encode()
    return files


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--source", type=Path, default=DEFAULT_SOURCE)
    args = ap.parse_args()
    if not (args.source / ".git").exists():
        print(f"sync-ocrcer: no OCRcer checkout at {args.source}", file=sys.stderr)
        return 1
    rev = head(args.source)
    files = expected_files(args.source, rev)
    if VENDOR.exists():
        shutil.rmtree(VENDOR)
    for rel, data in files.items():
        dest = VENDOR / rel
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(data)
    ADAPTER_DEST.write_bytes(show(args.source, rev, ADAPTER_SRC))
    print(f"sync-ocrcer: vendored OCRcer {rev[:12]} ({len(files)} files + adapter)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
