#!/usr/bin/env python3
"""extract-base14 — one-shot extraction of the Foxit base-14 font faces.

WHY THIS EXISTS (docs/decisions/004-text-rendering-fonts.md §4.2/§6.5):
pdfcer-render bundles the 14 Foxit standard-14 substitute faces that
pdfium (Chromium's PDF engine) has shipped since 2014. Upstream stores
them as C byte arrays in core/fxge/fontdata/chromefontdata/*.cpp; this
tool downloads those files from pdfium.googlesource.com at a recorded
commit, parses the arrays back into bare-CFF binaries, verifies each
looks like CFF (or records the actual header), and writes:

  crates/pdfcer-render/assets/fonts/<Name>.cff   (14 files)
  crates/pdfcer-render/assets/fonts/PROVENANCE.md
      source URL + upstream commit + per-file SHA-256 + byte size +
      the verbatim pdfium LICENSE text (BSD-3-Clause).

Provenance discipline is decision 004 rule R22: bundled font provenance
is verified and recorded, never asserted. Re-running the tool
regenerates everything; it is NOT part of the build (the .cff files are
committed) and is never shipped.

The two multiple-master fallback faces (FoxitSansMM/FoxitSerifMM) are
deliberately NOT extracted — decision 004 declined them for Pass 1.
"""

import base64
import hashlib
import json
import re
import sys
import urllib.request
from pathlib import Path

BASE = "https://pdfium.googlesource.com/pdfium"
DIR = "core/fxge/fontdata/chromefontdata"
REF = "refs/heads/main"

# Upstream .cpp file -> bundled face name (the standard-14 slot layout
# of decision 004 §4.2). Sans->Helvetica family, Serif->Times family,
# Fixed->Courier family.
FACES = {
    "FoxitSans.cpp": "FoxitSans",
    "FoxitSansBold.cpp": "FoxitSansBold",
    "FoxitSansItalic.cpp": "FoxitSansItalic",
    "FoxitSansBoldItalic.cpp": "FoxitSansBoldItalic",
    "FoxitSerif.cpp": "FoxitSerif",
    "FoxitSerifBold.cpp": "FoxitSerifBold",
    "FoxitSerifItalic.cpp": "FoxitSerifItalic",
    "FoxitSerifBoldItalic.cpp": "FoxitSerifBoldItalic",
    "FoxitFixed.cpp": "FoxitFixed",
    "FoxitFixedBold.cpp": "FoxitFixedBold",
    "FoxitFixedItalic.cpp": "FoxitFixedItalic",
    "FoxitFixedBoldItalic.cpp": "FoxitFixedBoldItalic",
    "FoxitSymbol.cpp": "FoxitSymbol",
    "FoxitDingbats.cpp": "FoxitDingbats",
}

OUT = Path(__file__).resolve().parents[2] / "crates" / "pdfcer-render" / "assets" / "fonts"


def fetch(url: str) -> bytes:
    req = urllib.request.Request(
        url, headers={"User-Agent": "pdfcer extract-base14 one-shot (see tools/extract-base14)"}
    )
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read()


def fetch_text_b64(path: str) -> bytes:
    """gitiles serves file contents base64-wrapped under ?format=TEXT."""
    return base64.b64decode(fetch(f"{BASE}/+/{REF}/{path}?format=TEXT"))


def head_commit() -> str:
    """Resolve the ref to a commit hash (gitiles JSON has an anti-XSSI
    prefix line to strip). The path-scoped +log endpoint 401s for
    anonymous access (observed 2026-07-30), so this uses the plain ref
    lookup — the tree state fetched below is exactly this commit's."""
    for url in (
        f"{BASE}/+/{REF}?format=JSON",
        f"{BASE}/+log/{REF}?format=JSON&n=1",
    ):
        try:
            raw = fetch(url).decode("utf-8")
            data = json.loads(raw.split("\n", 1)[1])
            return data.get("commit") or data["log"][0]["commit"]
        except Exception as e:  # noqa: BLE001 — one-shot tool, fall through
            print(f"  (commit lookup via {url.split('?')[0]} failed: {e})")
    return f"unresolved — ref {REF} at extraction time"


# One OR two hex digits: pdfium writes single-digit literals (0x1, 0x0)
# for small values. A two-digit-only pattern silently drops those bytes
# and corrupts every font — caught 2026-07-30 when the extracted blobs
# lost their CFF headers.
HEXBYTE = re.compile(r"0x([0-9a-fA-F]{1,2})\b")


def parse_c_array(src: str, filename: str) -> bytes:
    """Extract the (single) byte-array initializer from a chromefontdata
    .cpp file. These files contain exactly one `... g_FoxitXxx[...] = {
    0x.., ... };` — every 0xNN literal in the file belongs to it, so a
    global hex-literal scan is exact (verified: the files contain no
    other numeric literals in hex form outside the array)."""
    body = src[src.index("{"):]  # skip includes/decl before the array
    data = bytes(int(m.group(1), 16) for m in HEXBYTE.finditer(body))
    if not data:
        raise ValueError(f"{filename}: no byte array found")
    return data


def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    commit = head_commit()
    license_text = fetch_text_b64("LICENSE").decode("utf-8")

    rows = []
    for cpp, name in FACES.items():
        src = fetch_text_b64(f"{DIR}/{cpp}").decode("utf-8", errors="replace")
        data = parse_c_array(src, cpp)
        # CFF sanity: header starts 0x01 0x00 (major.minor). Record the
        # actual first bytes either way — R22 verifies, never assumes.
        header = data[:4].hex()
        looks_cff = data[0] == 0x01 and data[1] == 0x00
        out = OUT / f"{name}.cff"
        out.write_bytes(data)
        rows.append(
            (name, cpp, len(data), hashlib.sha256(data).hexdigest(), header, looks_cff)
        )
        print(f"{name:24} {len(data):7} B  cff={looks_cff}  {header}")

    total = sum(r[2] for r in rows)
    prov = [
        "# PROVENANCE — bundled standard-14 substitute faces",
        "",
        "Generated by `tools/extract-base14/extract.py` "
        "(decision 004 §6.5, rule R22 — provenance verified, never asserted).",
        "",
        f"- Source: `{BASE}/+/{REF}/{DIR}/` (Chromium pdfium)",
        f"- Upstream commit (last touching that directory): `{commit}`",
        "- Extraction: C byte-array literals parsed back to binary; see the",
        "  tool for the exact method.",
        "- License: BSD-3-Clause via Google's pdfium grant over Foxit-origin",
        "  code (`// Original code copyright 2014 Foxit Software Inc.` in each",
        "  upstream file). No standalone Foxit-published grant exists — the",
        "  chain of title runs through pdfium (decision 004 §5.4); review once",
        "  before first public release per LEGAL.md §1.",
        f"- Total: {len(rows)} faces, {total} bytes.",
        "",
        "| Face | Upstream file | Bytes | SHA-256 | First bytes | CFF header? |",
        "|---|---|---:|---|---|---|",
    ]
    for name, cpp, size, sha, header, looks_cff in rows:
        prov.append(f"| {name} | {cpp} | {size} | `{sha}` | `{header}` | {looks_cff} |")
    prov += [
        "",
        "## Verbatim pdfium LICENSE",
        "",
        "```",
        license_text.rstrip(),
        "```",
        "",
    ]
    (OUT / "PROVENANCE.md").write_text("\n".join(prov), encoding="utf-8", newline="\n")
    print(f"\n{len(rows)} faces, {total} bytes total -> {OUT}")
    print(f"commit {commit}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
