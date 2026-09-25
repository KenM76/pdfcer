#!/usr/bin/env python3
"""Build the Tesseract bundle that ships as `models/tesseract` (Windows x64).

Output: target/tesseract-bundle/
    tesseract.exe                one static MSVC executable, no DLLs
    tessdata/<lang>.traineddata  pinned tessdata_fast files, SHA-256 checked
    LICENSES/<package>.txt       the licence of every library linked in
    PROVENANCE.md                versions, pins and build options

`tools/package-portable.py` copies this folder into the release.

What makes the build "clean" (see README.md here):
- an overlay of vcpkg's tesseract port with curl, libarchive and the
  ScrollView debug viewer disabled, so no LGPL library and no socket code;
- a static, release-only triplet with the static CRT, so the exe imports
  only Windows system DLLs. This script REFUSES the bundle otherwise.

Needs: git, Visual Studio 2022 C++ build tools, network access (vcpkg
sources and the language files). Usage:
    python tools/tesseract/build-tesseract.py [--langs eng+deu]
"""

from __future__ import annotations

import argparse
import hashlib
import shutil
import struct
import subprocess
import sys
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
WORK = REPO / "target" / "tesseract-build"
VCPKG = WORK / "vcpkg"
INSTALLED = WORK / "installed"
OUT = REPO / "target" / "tesseract-bundle"

VCPKG_REPO = "https://github.com/microsoft/vcpkg.git"
#: vcpkg commit whose port versions this bundle is built from.
VCPKG_COMMIT = "18ff00a7d2697e23f22e2a2293c3fa6468a50c06"
TRIPLET = "x64-windows-static-release"

TESSDATA_TAG = "4.1.0"
TESSDATA_URL = "https://github.com/tesseract-ocr/tessdata_fast/raw/{tag}/{lang}.traineddata"
#: SHA-256 of each language file this script knows. A language not listed is
#: refused rather than fetched unchecked; add its hash after verifying it.
TESSDATA_SHA256 = {
    "eng": "7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2",
}

#: DLLs every Windows install carries. Anything else imported is a DLL the
#: bundle would have to ship, which defeats the static build.
SYSTEM_DLLS = {"kernel32.dll", "user32.dll", "advapi32.dll", "bcrypt.dll", "ntdll.dll"}

#: vcpkg packages that are build tools, not linked code.
BUILD_ONLY = {"vcpkg-cmake", "vcpkg-cmake-config"}


def run(cmd: list[str], **kw) -> None:
    print("+", " ".join(cmd), flush=True)
    subprocess.run(cmd, check=True, **kw)


def pe_imports(exe: Path) -> list[str]:
    """DLL names in a PE32+ file's import directory."""
    data = exe.read_bytes()
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe : pe + 4] != b"PE\0\0":
        raise ValueError(f"{exe}: not a PE file")
    n_sections = struct.unpack_from("<H", data, pe + 6)[0]
    opt_size = struct.unpack_from("<H", data, pe + 20)[0]
    opt = pe + 24
    if struct.unpack_from("<H", data, opt)[0] != 0x20B:
        raise ValueError(f"{exe}: not PE32+")
    import_rva = struct.unpack_from("<I", data, opt + 112 + 8)[0]
    sections = []
    for i in range(n_sections):
        s = opt + opt_size + 40 * i
        vsize, va, rsize, raw = struct.unpack_from("<IIII", data, s + 8)
        sections.append((va, max(vsize, rsize), raw))

    def off(rva: int) -> int:
        for va, size, raw in sections:
            if va <= rva < va + size:
                return rva - va + raw
        raise ValueError(f"RVA {rva:#x} outside every section")

    names, desc = [], off(import_rva)
    while True:
        name_rva = struct.unpack_from("<I", data, desc + 12)[0]
        if name_rva == 0:
            return names
        start = off(name_rva)
        names.append(data[start : data.index(b"\0", start)].decode("ascii"))
        desc += 20


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--langs", default="eng", help="languages to bundle, joined by '+'")
    args = ap.parse_args()
    langs = args.langs.split("+")
    unknown = [lang for lang in langs if lang not in TESSDATA_SHA256]
    if unknown:
        print(f"build-tesseract: no pinned SHA-256 for {', '.join(unknown)}; add it to TESSDATA_SHA256")
        return 2
    if sys.platform != "win32":
        print("build-tesseract: the bundle is Windows x64 only")
        return 2

    WORK.mkdir(parents=True, exist_ok=True)
    if not VCPKG.is_dir():
        run(["git", "clone", "--quiet", VCPKG_REPO, str(VCPKG)])
    run(["git", "-C", str(VCPKG), "fetch", "--quiet", "origin", VCPKG_COMMIT])
    run(["git", "-C", str(VCPKG), "checkout", "--quiet", "--detach", VCPKG_COMMIT])
    exe_vcpkg = VCPKG / "vcpkg.exe"
    if not exe_vcpkg.is_file():
        run(
            ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File",
             str(VCPKG / "scripts" / "bootstrap.ps1"), "-disableMetrics"]
        )

    common = [
        f"--triplet={TRIPLET}",
        f"--overlay-ports={HERE / 'overlay-ports'}",
        f"--overlay-triplets={HERE / 'triplets'}",
        f"--x-install-root={INSTALLED}",
        "--disable-metrics",
    ]
    # Remove first: vcpkg treats an installed package as done even when the
    # overlay port changed.
    subprocess.run([str(exe_vcpkg), "remove", "--recurse", "tesseract", *common], check=False)
    run([str(exe_vcpkg), "install", "tesseract", *common])

    prefix = INSTALLED / TRIPLET
    built = prefix / "tools" / "tesseract" / "tesseract.exe"
    imports = pe_imports(built)
    foreign = [d for d in imports if d.lower() not in SYSTEM_DLLS]
    if foreign:
        print(f"build-tesseract: REFUSED — tesseract.exe imports non-system DLLs: {foreign}")
        return 1

    if OUT.exists():
        shutil.rmtree(OUT)
    (OUT / "tessdata").mkdir(parents=True)
    (OUT / "LICENSES").mkdir()
    shutil.copy2(built, OUT / "tesseract.exe")

    for lang in langs:
        url = TESSDATA_URL.format(tag=TESSDATA_TAG, lang=lang)
        print("+ fetch", url, flush=True)
        with urllib.request.urlopen(url) as r:  # noqa: S310 - pinned https URL, hash-checked below
            body = r.read()
        digest = hashlib.sha256(body).hexdigest()
        if digest != TESSDATA_SHA256[lang]:
            print(f"build-tesseract: REFUSED — {lang}.traineddata SHA-256 {digest} != pinned")
            return 1
        (OUT / "tessdata" / f"{lang}.traineddata").write_bytes(body)

    packages = []
    for share in sorted((prefix / "share").iterdir()):
        copyright_file = share / "copyright"
        if share.name in BUILD_ONLY or not copyright_file.is_file():
            continue
        shutil.copy2(copyright_file, OUT / "LICENSES" / f"{share.name}.txt")
        packages.append(share.name)
    shutil.copy2(HERE / "tessdata_fast-LICENSE.txt", OUT / "LICENSES" / "tessdata_fast.txt")

    version = subprocess.run(
        [str(OUT / "tesseract.exe"), "--version"], capture_output=True, text=True, check=True
    ).stdout.strip()
    provenance = f"""# models/tesseract — provenance and licensing

Tesseract OCR, Apache-2.0, built by pdfcer's `tools/tesseract/build-tesseract.py`
from vcpkg commit `{VCPKG_COMMIT}`, triplet `{TRIPLET}` (static libraries,
static MSVC runtime), with the overlay port in `tools/tesseract/overlay-ports`:
curl, libarchive and the ScrollView viewer disabled. `tesseract.exe` imports
only: {", ".join(imports)}.

```
{version}
```

Linked libraries and their licences are in `LICENSES/` — one file per vcpkg
package: {", ".join(packages)}. All are permissive (Apache-2.0, BSD, MIT,
zlib, libpng, libtiff, IJG); none is copyleft.

Language data: `tessdata/` from tesseract-ocr/tessdata_fast tag
`{TESSDATA_TAG}`, Apache-2.0 (`LICENSES/tessdata_fast.txt`):
{chr(10).join(f"- {lang}.traineddata  sha256 {TESSDATA_SHA256[lang]}" for lang in langs)}

More languages: copy `<code>.traineddata` from tessdata_fast, tessdata or
tessdata_best into `tessdata/` and pass `--ocr-lang`.
"""
    (OUT / "PROVENANCE.md").write_text(provenance, encoding="utf-8")
    total = sum(f.stat().st_size for f in OUT.rglob("*") if f.is_file())
    print(f"build-tesseract: bundle at {OUT} ({total / 1e6:.1f} MB), imports {imports}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
