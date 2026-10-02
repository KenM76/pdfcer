"""Write `pdfcer-ocr-model.txt` for a Tesseract folder, making it a program
OCR add-on (decision 184): `kind = program`, the program's SHA-256, and the
SHA-256 of every `tessdata/*.traineddata` present.

pdfcer runs the program only when its hash matches, and re-checks every
listed file before each page. Run this again after adding a language file,
or the new file is used unverified.

Usage: python tools/tesseract/write-ocr-manifest.py <folder> [--name NAME]
"""

from __future__ import annotations

import argparse
import hashlib
import sys
from pathlib import Path

MANIFEST = "pdfcer-ocr-model.txt"
TESSDATA = "tessdata"


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def manifest_text(folder: Path, name: str) -> str:
    exe = next(
        (folder / n for n in ("tesseract.exe", "tesseract") if (folder / n).is_file()),
        None,
    )
    if exe is None:
        raise SystemExit(f"write-ocr-manifest: no tesseract program in {folder}")
    langs = sorted((folder / TESSDATA).glob("*.traineddata"))
    if not langs:
        raise SystemExit(f"write-ocr-manifest: no {TESSDATA}/*.traineddata in {folder}")
    lines = [
        "# pdfcer OCR model add-on manifest (decisions 182, 184). Written by",
        "# tools/tesseract/write-ocr-manifest.py; see PROVENANCE.md.",
        f"name = {name}",
        "engine = tesseract",
        "kind = program",
        f"program = {exe.name}",
        f"data = {TESSDATA}",
        "label = Tesseract",
        "languages = " + ", ".join(p.stem for p in langs),
        "licence = Apache-2.0",
        f"sha256 = {exe.name} {sha256(exe)}",
    ]
    lines += [f"sha256 = {TESSDATA}/{p.name} {sha256(p)}" for p in langs]
    return "\n".join(lines) + "\n"


def write_manifest(folder: Path, name: str = "tesseract") -> Path:
    out = folder / MANIFEST
    out.write_bytes(manifest_text(folder, name).encode("utf-8"))
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("folder", type=Path)
    ap.add_argument("--name", default="tesseract")
    args = ap.parse_args()
    out = write_manifest(args.folder, args.name)
    print(f"write-ocr-manifest: wrote {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
