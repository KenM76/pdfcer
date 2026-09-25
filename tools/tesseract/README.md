# Bundled Tesseract (`models/tesseract`)

`pdfcer ocr --ocr-engine tesseract` runs Tesseract as a separate program.
pdfcer does not link it: the page goes in on stdin as a PGM image, and TSV
comes back on stdout. The portable package ships a build made here.

## Build

```
python tools/tesseract/build-tesseract.py            # eng only
python tools/tesseract/build-tesseract.py --langs eng+deu   # after pinning deu's hash
```

Needs git, the Visual Studio 2022 C++ build tools (CMake and Ninja come
with them) and network access. The first run takes about 15 minutes. Output
goes to `target/tesseract-bundle/`, and `tools/package-portable.py` picks it
up. If there is no bundle, the package still builds and prints a warning;
`--ocr-engine tesseract` then needs `--model-dir`.

## Why this build rather than a downloaded one

The common Windows builds (UB-Mannheim, MSYS2) are MinGW builds that ship
dozens of DLLs, some under weak-copyleft terms. pdfcer does not decide LGPL
questions alone (`LEGAL.md` §6.1), so this build links only what OCR needs
and makes every piece of it permissive:

| Choice | Where | Effect |
|---|---|---|
| `DISABLE_CURL=ON` | overlay port | no URL input, no TLS stack |
| `DISABLE_ARCHIVE=ON` | overlay port | no libarchive (which pulls LGPL codecs) |
| `GRAPHICS_DISABLED=ON` | overlay port | no ScrollView debug viewer; without this the exe imports `WS2_32.dll` for its socket |
| training tools off | overlay port | only `tesseract.exe` is built |
| `x64-windows-static-release` | `triplets/` | static libraries and static CRT: one exe, no DLLs, no VC++ redistributable |

The script parses the exe's import table and **refuses** the bundle if
it imports anything beyond core Windows DLLs. The current build imports only
`KERNEL32.dll`. Everything linked in is permissive: Apache-2.0, BSD, MIT,
zlib, libpng, libtiff or IJG. The build writes one licence file per library
to `LICENSES/`.

Two licence files will trip a keyword scan, but both are fine. liblzma's file
mentions GPL/LGPL only for xz's command-line tools and scripts, which are not
built; liblzma itself is 0BSD. libspng's file is long because vcpkg's
copyright file includes source; its licence is BSD-2.

## Pins

- vcpkg commit `VCPKG_COMMIT` in the script, which pins tesseract 5.5.2 and
  Leptonica 1.87.0.
- `tessdata_fast` tag 4.1.0, with the SHA-256 of each language file in
  `TESSDATA_SHA256`. A language without a pinned hash is refused.

To bump a version, move `VCPKG_COMMIT`, re-copy `ports/tesseract` from that
commit into `overlay-ports/tesseract/`, and re-apply the changes listed
above. Those changes are the `DISABLE_*`/`GRAPHICS_DISABLED` options, the
dependency list in `vcpkg.json`, and the `find_dependency` lines for CURL and
LibArchive.

## Gotchas

- vcpkg treats an installed package as done even after the overlay changes,
  so the script always runs `vcpkg remove` first.
- `bootstrap-vcpkg.bat` does not run through `cmd //c` from Git Bash, so the
  script calls `scripts/bootstrap.ps1` directly.
- Tesseract's `tsv` config name needs `tessdata/configs/tsv`, which the
  bundle leaves out. pdfcer passes `-c tessedit_create_tsv=1` instead.

## Prior art

NAPS2 also ships a static MSVC `tesseract.exe` and runs it as a subprocess
(its build still links curl and libarchive). Most other apps either ship the
MinGW DLL set or require a system install (OCRmyPDF, Paperless-ngx,
Stirling-PDF).
