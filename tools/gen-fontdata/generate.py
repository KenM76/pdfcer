#!/usr/bin/env python3
"""Generate crates/pdfcer-core/src/fontdata/tables.rs from the PDF-spec RAG.

WHAT THIS IS
------------
The single source of the standard-14 font data compiled into pdfcer-core:
per-glyph advance widths, /FontDescriptor payloads, the Annex D.2
predefined encodings, the Symbol/ZapfDingbats built-in encodings, and the
glyph-name -> Unicode subset used for text extraction. The generated file
is COMMITTED (the RAG is a private corpus outside the repo; CI and other
machines must build without it), and this script is the only legitimate
way to change it (pdfcer decision 004 s10 item 6: "generate the std-14
width/descriptor tables and the Annex D encoding tables from the staged
RAG sources").

INPUTS (dense markdown data tables in the private spec RAG)
-----------------------------------------------------------
  D:/Dev/Rag-Specialized/PDF_Spec/
    fonts/font__std14_widths__helvetica.md   315 glyphs x (Helv, Helv-Bd)
                                             + STD/MAC/WIN octal code columns
    fonts/font__std14_widths__times.md       315 glyphs x 4 face columns
    fonts/font__std14_widths__courier.md     deliberately tableless: EVERY
                                             glyph in all 4 faces is 600;
                                             repertoire == Helvetica's names
    fonts/font__std14_widths__symbol.md      189 codes -> name/width/Unicode
    fonts/font__std14_widths__zapfdingbats.md 202 codes -> name/width/Unicode
    fonts/font__std14_descriptors.md         FontBBox/Ascender/... x 14 fonts
                                             + the derived Table-123 Flags
    iso32000/iso32000__annex__d.md           229-row Standard/MacRoman/WinAnsi
                                             encoding table (octal) + footnotes
    fonts/font__agl.md                       315 Latin glyph names -> Unicode

ALGORITHM
---------
1. Parse each markdown table by shape (cell count + backticked first/name
   cell), never by section heading, so cosmetic RAG edits don't break the
   parse. All code values in the RAG are OCTAL (as in ISO 32000-1 Annex D)
   and are converted here exactly once.
2. Cross-validate everything that appears in two places (this is the
   defence against extraction drift):
     - Helvetica/Times name sets are identical (the shared 315-name Latin
       repertoire, which is also Courier's).
     - The STD/MAC/WIN convenience columns in the width files match the
       authoritative Annex D table, name by name.
     - The WINdec column matches the octal WIN column.
     - Annex D per-encoding assignment counts are exactly 149/207/216
       (the published sizes, quoted in the RAG).
     - The AGL file covers all 315 Latin names; the symbolic fonts' name
       sets carry their own Unicode columns; merged duplicate names must
       agree on the code point (e.g. `Delta` = U+2206 in both Latin and
       Symbol).
     - Symbol/ZapfDingbats code tables have no duplicate codes or names.
   Any mismatch aborts with a message naming the offending row - the RAG
   is authoritative; this script never "fixes" data.
3. Apply the Annex D.2 footnote entries that are NOT table rows:
     - WinAnsi 0o240 -> space (nonbreaking), 0o255 -> hyphen (soft hyphen)
     - MacRoman 0o312 -> space (nonbreaking)
     - WinAnsi: every remaining unassigned code >= 0o40 -> bullet
       (footnote 3; this is why PDF's WinAnsi is NOT CP1252)
4. Emit tables.rs: sorted, deterministic (no timestamps), each data item
   under #[rustfmt::skip] so `cargo fmt` never reflows it and a rerun of
   this script is byte-identical. Lookup tables are sorted by glyph name
   (ASCII byte order == Rust &str Ord) for binary_search_by in mod.rs.

LICENSING (why the header block below exists)
---------------------------------------------
The width/descriptor numbers derive from the Adobe Core 14 AFM files,
licensed APAFML: the copyright notice must be retained and modifications
prominently noted - both done in the generated header (see
font__std14_afm_licensing.md for the verdict and obligations; pdfcer ships
extracted numbers, never the .afm files themselves). The Unicode mappings
derive from the Adobe Glyph List, BSD-3-Clause: notice retention likewise
satisfied in the header. Both also need manual THIRD_PARTY_LICENSES.md
entries (cargo-about cannot see non-Cargo data dependencies).

USAGE
-----
    python tools/gen-fontdata/generate.py

Exit 0 on success (prints a row-count summary); exit 1 with a diagnostic
on any validation failure. Requires the RAG checkout at the path above;
no third-party Python packages.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
RAG = Path(r"D:/Dev/Rag-Specialized/PDF_Spec")
OUT = REPO / "crates" / "pdfcer-core" / "src" / "fontdata" / "tables.rs"

DASH = "\u2014"  # em dash: the RAG's "unencoded / absent" marker
NAME_RE = re.compile(r"^`([^`]+)`$")
UNI_RE = re.compile(r"^U\+([0-9A-F]{4,6})$")

# BaseFont name -> Rust enum variant, in enum declaration order.
VARIANTS = {
    "Helvetica": "Helvetica",
    "Helvetica-Bold": "HelveticaBold",
    "Helvetica-Oblique": "HelveticaOblique",
    "Helvetica-BoldOblique": "HelveticaBoldOblique",
    "Times-Roman": "TimesRoman",
    "Times-Bold": "TimesBold",
    "Times-Italic": "TimesItalic",
    "Times-BoldItalic": "TimesBoldItalic",
    "Courier": "Courier",
    "Courier-Bold": "CourierBold",
    "Courier-Oblique": "CourierOblique",
    "Courier-BoldOblique": "CourierBoldOblique",
    "Symbol": "Symbol",
    "ZapfDingbats": "ZapfDingbats",
}


def fail(msg: str) -> None:
    print(f"ERROR: {msg}", file=sys.stderr)
    sys.exit(1)


def table_rows(path: Path) -> list[list[str]]:
    """All pipe-table rows of a markdown file as stripped cell lists.

    Separator rows (`|---|---|`) are dropped. Callers filter by shape:
    cell count plus a backticked name cell uniquely identifies each data
    table in these files (verified when this generator was written).
    """
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line.startswith("|"):
            continue
        cells = [c.strip() for c in line.strip("|").split("|")]
        if all(re.fullmatch(r":?-{3,}:?", c) for c in cells if c):
            continue
        rows.append(cells)
    return rows


def strip_md(cell: str) -> str:
    """Remove bold markers and backticks from a table cell."""
    return cell.replace("**", "").replace("`", "").strip()


def octal_or_none(cell: str, where: str) -> int | None:
    cell = strip_md(cell)
    if cell == DASH:
        return None
    try:
        value = int(cell, 8)
    except ValueError:
        fail(f"{where}: expected octal or dash, got {cell!r}")
    if not 0 <= value <= 0o377:
        fail(f"{where}: octal code {cell} out of byte range")
    return value


# --------------------------------------------------------------------------
# Parsers
# --------------------------------------------------------------------------

def parse_latin_widths() -> dict[str, dict]:
    """Merge the Helvetica and Times width tables into one 315-name map.

    Result: name -> {std, mac, win (Optional[int]), helv, helv_bd,
    times_r, times_b, times_i, times_bi}. The Courier file is deliberately
    tableless (every glyph 600); its repertoire is this same name set.
    """
    helv: dict[str, dict] = {}
    for cells in table_rows(RAG / "fonts" / "font__std14_widths__helvetica.md"):
        if len(cells) != 7:
            continue
        m = NAME_RE.match(cells[0])
        if not m:
            continue
        name = m.group(1)
        where = f"helvetica:{name}"
        std, mac, win = (octal_or_none(cells[i], where) for i in (1, 2, 3))
        windec = None if strip_md(cells[4]) == DASH else int(strip_md(cells[4]))
        if (win is None) != (windec is None) or (win is not None and win != windec):
            fail(f"{where}: WIN octal {cells[3]} != WINdec {cells[4]}")
        helv[name] = {
            "std": std, "mac": mac, "win": win,
            "helv": int(cells[5]), "helv_bd": int(cells[6]),
        }

    times: dict[str, dict] = {}
    for cells in table_rows(RAG / "fonts" / "font__std14_widths__times.md"):
        if len(cells) != 9:
            continue
        m = NAME_RE.match(cells[0])
        if not m:
            continue
        name = m.group(1)
        where = f"times:{name}"
        std, mac, win = (octal_or_none(cells[i], where) for i in (1, 2, 3))
        times[name] = {
            "std": std, "mac": mac, "win": win,
            "times_r": int(cells[5]), "times_b": int(cells[6]),
            "times_i": int(cells[7]), "times_bi": int(cells[8]),
        }

    if len(helv) != 315:
        fail(f"helvetica width table: expected 315 rows, got {len(helv)}")
    if len(times) != 315:
        fail(f"times width table: expected 315 rows, got {len(times)}")
    if set(helv) != set(times):
        diff = set(helv) ^ set(times)
        fail(f"helvetica/times name sets differ: {sorted(diff)}")

    merged = {}
    for name, h in helv.items():
        t = times[name]
        for key in ("std", "mac", "win"):
            if h[key] != t[key]:
                fail(f"{name}: {key} code differs between helvetica and times files")
        merged[name] = {**h, **t}
    return merged


def parse_symbolic(filename: str, expected: int) -> list[dict]:
    """Parse a Symbol/ZapfDingbats table: code(oct) code(dec) name width U+X."""
    out = []
    for cells in table_rows(RAG / "fonts" / filename):
        if len(cells) != 5:
            continue
        m = NAME_RE.match(cells[2])
        if not m:
            continue
        name = m.group(1)
        where = f"{filename}:{name}"
        try:
            code_oct = int(cells[0], 8)
            code_dec = int(cells[1])
        except ValueError:
            continue  # header row
        if code_oct != code_dec:
            fail(f"{where}: octal {cells[0]} != decimal {cells[1]}")
        u = UNI_RE.match(cells[4])
        if not u:
            fail(f"{where}: bad Unicode cell {cells[4]!r}")
        out.append({
            "code": code_dec, "name": name,
            "width": int(cells[3]), "unicode": int(u.group(1), 16),
        })
    if len(out) != expected:
        fail(f"{filename}: expected {expected} rows, got {len(out)}")
    codes = [r["code"] for r in out]
    names = [r["name"] for r in out]
    if len(set(codes)) != len(codes) or len(set(names)) != len(names):
        fail(f"{filename}: duplicate code or name")
    return out


def parse_annex_d() -> dict[str, dict]:
    """Annex D.2: name -> {std, mac, win} (Optional[int] each). 229 rows."""
    out: dict[str, dict] = {}
    for cells in table_rows(RAG / "iso32000" / "iso32000__annex__d.md"):
        if len(cells) != 6:
            continue
        m = NAME_RE.match(cells[0])
        if not m:
            continue
        name = m.group(1)
        where = f"annex_d:{name}"
        std, mac, win = (octal_or_none(cells[i], where) for i in (1, 2, 3))
        windec = None if strip_md(cells[5]) == DASH else int(strip_md(cells[5]))
        if (win is None) != (windec is None) or (win is not None and win != windec):
            fail(f"{where}: WIN octal {cells[3]} != WIN(dec) {cells[5]}")
        out[name] = {"std": std, "mac": mac, "win": win}
    if len(out) != 229:
        fail(f"annex D: expected 229 rows, got {len(out)}")
    counts = {
        k: sum(1 for v in out.values() if v[k] is not None)
        for k in ("std", "mac", "win")
    }
    if counts != {"std": 149, "mac": 207, "win": 216}:
        fail(f"annex D column counts {counts} != published 149/207/216")
    return out


def parse_agl_latin() -> dict[str, int]:
    """font__agl.md's 315-name Latin table: name -> code point."""
    out: dict[str, int] = {}
    for cells in table_rows(RAG / "fonts" / "font__agl.md"):
        if len(cells) != 4:
            continue
        m = NAME_RE.match(cells[0])
        u = UNI_RE.match(cells[1])
        if not (m and u):
            continue  # e.g. the data-files table (backticked name, no U+)
        out[m.group(1)] = int(u.group(1), 16)
    if len(out) != 315:
        fail(f"font__agl.md Latin table: expected 315 rows, got {len(out)}")
    return out


def parse_descriptors() -> dict[str, dict]:
    """font__std14_descriptors.md: per-font Table 122 values + derived Flags.

    Symbol/ZapfDingbats have no Ascender/Descender/CapHeight/XHeight in
    their AFMs (the RAG marks the cells with an em dash). Per the RAG's
    preferred derivation (its option 2), ascent/descent fall back to the
    FontBBox ury/lly, and cap/x height to 0 (Table 122 default;
    CapHeight is required only "for fonts that have Latin characters").
    """
    path = RAG / "fonts" / "font__std14_descriptors.md"
    rows = table_rows(path)

    flags: dict[str, int] = {}
    for cells in rows:
        if len(cells) != 3:
            continue
        fm = re.fullmatch(r"\*\*(\d+)\*\*", cells[2])
        if not fm:
            continue
        for font in re.findall(r"`([A-Za-z-]+)`", cells[0]):
            if font in VARIANTS:
                flags[font] = int(fm.group(1))
    if set(flags) != set(VARIANTS):
        fail(f"descriptor Flags table: fonts {sorted(set(VARIANTS) - set(flags))} missing")

    out: dict[str, dict] = {}
    for cells in rows:
        if len(cells) != 13:
            continue
        m = NAME_RE.match(cells[0])
        if not m or m.group(1) not in VARIANTS:
            continue
        font = m.group(1)
        where = f"descriptors:{font}"
        bm = re.fullmatch(
            r"\[(-?\d+) (-?\d+) (-?\d+) (-?\d+)\]", strip_md(cells[1])
        )
        if not bm:
            fail(f"{where}: bad FontBBox cell {cells[1]!r}")
        bbox = [int(g) for g in bm.groups()]

        def metric(i: int, absent: int) -> int:
            v = strip_md(cells[i])
            return absent if v == DASH else int(v)

        italic = strip_md(cells[6])
        out[font] = {
            "bbox": bbox,
            # Absent (symbolic fonts): bbox-derived ascent/descent, 0 heights.
            "asc": metric(2, bbox[3]),
            "desc": metric(3, bbox[1]),
            "cap": metric(4, 0),
            "x": metric(5, 0),
            "italic": float(italic),
            "stem_v": int(strip_md(cells[7])),
            "flags": flags[font],
            "afm_version": strip_md(cells[11]),
        }
    if set(out) != set(VARIANTS):
        fail(f"descriptor table: fonts {sorted(set(VARIANTS) - set(out))} missing")
    return out


# --------------------------------------------------------------------------
# Cross-validation + assembly
# --------------------------------------------------------------------------

def build() -> dict:
    latin = parse_latin_widths()
    symbol = parse_symbolic("font__std14_widths__symbol.md", 189)
    zapf = parse_symbolic("font__std14_widths__zapfdingbats.md", 202)
    annex = parse_annex_d()
    agl_latin = parse_agl_latin()
    descriptors = parse_descriptors()

    # The 229 Annex D names are a subset of the 315-name repertoire, and
    # the width files' convenience code columns must agree with Annex D
    # (the authority). Names outside Annex D must be dash-only.
    if not set(annex) <= set(latin):
        fail(f"annex D names outside the Latin repertoire: {sorted(set(annex) - set(latin))}")
    for name, row in latin.items():
        expect = annex.get(name, {"std": None, "mac": None, "win": None})
        for key in ("std", "mac", "win"):
            if row[key] != expect[key]:
                fail(f"{name}: width-file {key} code {row[key]} != annex D {expect[key]}")

    if set(agl_latin) != set(latin):
        fail("font__agl.md Latin names differ from the width repertoire")

    # Merge the three Unicode sources; duplicates must agree (Delta, mu,
    # space, the shared punctuation, ...). ZapfDingbats' aNN names enter
    # the same table — the documented merged-table deviation in mod.rs.
    glyph_to_unicode: dict[str, int] = dict(agl_latin)
    for source, rows in (("symbol", symbol), ("zapfdingbats", zapf)):
        for r in rows:
            prev = glyph_to_unicode.get(r["name"])
            if prev is not None and prev != r["unicode"]:
                fail(
                    f"{source}:{r['name']}: Unicode U+{r['unicode']:04X} "
                    f"conflicts with U+{prev:04X} from another table"
                )
            glyph_to_unicode[r["name"]] = r["unicode"]

    # Encoding arrays (index = byte code, value = glyph name or None).
    def invert(key: str) -> list[str | None]:
        enc: list[str | None] = [None] * 256
        for name, row in annex.items():
            code = row[key]
            if code is None:
                continue
            if enc[code] is not None:
                fail(f"{key} encoding: code {code} assigned twice ({enc[code]}, {name})")
            enc[code] = name
        return enc

    std_enc = invert("std")
    mac_enc = invert("mac")
    win_enc = invert("win")

    # Annex D.2 footnote entries (not table rows — see module docs):
    # fn 6: nonbreaking space; fn 5: soft hyphen; fn 3: WinAnsi bullet fill.
    for code, name in ((0o240, "space"), (0o255, "hyphen")):
        if win_enc[code] is not None:
            fail(f"WinAnsi footnote code {code:#o} unexpectedly already assigned")
        win_enc[code] = name
    if mac_enc[0o312] is not None:
        fail("MacRoman footnote code 0o312 unexpectedly already assigned")
    mac_enc[0o312] = "space"
    bullet_fill = [c for c in range(0o40, 256) if win_enc[c] is None]
    for code in bullet_fill:
        win_enc[code] = "bullet"

    def symbolic_enc(rows: list[dict]) -> list[str | None]:
        enc: list[str | None] = [None] * 256
        for r in rows:
            enc[r["code"]] = r["name"]
        return enc

    return {
        "latin": latin,
        "symbol": symbol,
        "zapf": zapf,
        "std_enc": std_enc,
        "mac_enc": mac_enc,
        "win_enc": win_enc,
        "symbol_enc": symbolic_enc(symbol),
        "zapf_enc": symbolic_enc(zapf),
        "glyph_to_unicode": glyph_to_unicode,
        "descriptors": descriptors,
        "bullet_fill": bullet_fill,
    }


# --------------------------------------------------------------------------
# Emission
# --------------------------------------------------------------------------

APAFML = """\
Copyright (c) 1985, 1987, 1989, 1990, 1991, 1992, 1993, 1997 Adobe Systems
Incorporated.  All Rights Reserved.

This file and the 14 PostScript(R) AFM files it accompanies may be used,
copied, and distributed for any purpose and without charge, with or
without modification, provided that all copyright notices are retained;
that the AFM files are not distributed without this file; that all
modifications to this file or any of the AFM files are prominently noted
in the modified file(s); and that this paragraph is not modified. Adobe
Systems has no responsibility or obligation to support the use of the AFM
files."""

BSD3 = """\
Copyright 2002-2019 Adobe (http://www.adobe.com/)

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

1. Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the distribution.

3. Neither the name of Adobe nor the names of its contributors may be
   used to endorse or promote products derived from this software
   without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE."""


def comment_block(text: str, prefix: str = "// ") -> str:
    return "\n".join(
        (prefix + line).rstrip() for line in text.splitlines()
    )


def enc_lines(enc: list[str | None]) -> list[str]:
    """Render a 256-entry encoding array body: Some() entries one per line
    with an index comment; runs of None packed 8 per line with a range
    comment. Deterministic; #[rustfmt::skip] preserves the layout."""
    lines: list[str] = []
    i = 0
    while i < 256:
        if enc[i] is None:
            start = i
            while i < 256 and enc[i] is None:
                i += 1
            run = i - start
            for chunk_start in range(start, i, 8):
                n = min(8, i - chunk_start)
                lines.append(
                    "    " + "None, " * (n - 1) + "None,"
                    + f" // {chunk_start:#04x}..={chunk_start + n - 1:#04x}"
                )
            _ = run
        else:
            lines.append(f'    Some("{enc[i]}"), // {i:#04x} = {i:#o}')
            i += 1
    return lines


def emit(data: dict) -> str:
    latin = data["latin"]
    descriptors = data["descriptors"]
    lines: list[str] = []
    push = lines.append

    latin_names = sorted(latin)
    symbol_sorted = sorted(data["symbol"], key=lambda r: r["name"])
    zapf_sorted = sorted(data["zapf"], key=lambda r: r["name"])
    agl_sorted = sorted(data["glyph_to_unicode"].items())

    afm_versions = ", ".join(
        f"{font} {descriptors[font]['afm_version']}" for font in VARIANTS
    )

    push("//! GENERATED FILE — DO NOT EDIT BY HAND.")
    push("//!")
    push("//! Generated by `tools/gen-fontdata/generate.py` (rerunnable,")
    push("//! deterministic — regenerate rather than editing) from the staged")
    push("//! PDF-spec RAG sources:")
    push("//!")
    push("//! - `fonts/font__std14_widths__{helvetica,times,courier,symbol,zapfdingbats}.md`")
    push("//! - `fonts/font__std14_descriptors.md`")
    push("//! - `iso32000/iso32000__annex__d.md` (ISO 32000-1 Annex D.2)")
    push("//! - `fonts/font__agl.md` (Adobe Glyph List subset)")
    push("//!")
    push("//! under `D:\\Dev\\Rag-Specialized\\PDF_Spec\\`. See `mod.rs` for the")
    push("//! data contracts and `tools/gen-fontdata/generate.py` for the")
    push("//! parsing/validation algorithm. Spec basis: ISO 32000-1 §9.6.2.2")
    push("//! (standard 14), §9.2.4 (glyph space, 1000/em), §9.6.6.1 (built-in")
    push("//! encodings), §9.8.2 Tables 122/123 (descriptors), Annex D.2.")
    push("//!")
    push("//! ## Derivation / modification notice (APAFML obligation)")
    push("//!")
    push("//! The width and descriptor values are DERIVED from the Adobe Core 14")
    push("//! AFM files: only `WX` advance widths and global header metrics were")
    push("//! extracted; kerning pairs (`KPX`), per-glyph bounding boxes (`B`),")
    push("//! ligature data (`L`) and composites were discarded. `/Flags` values")
    push("//! and the Symbol/ZapfDingbats Ascent/Descent are derived, not read")
    push("//! from any AFM (see `font__std14_descriptors.md`). AFM `Version`")
    push(f"//! per font: {afm_versions}.")
    push("//!")
    push("//! ## License notices (retained per APAFML and BSD-3-Clause)")
    push("//!")
    push("//! Width, encoding and descriptor data: Adobe Core 14 AFM files,")
    push("//! licensed under APAFML (SPDX: APAFML). The license text, verbatim:")
    push("//!")
    push(comment_block(APAFML, "//! > "))
    push("//!")
    push("//! Glyph-name → Unicode data: Adobe Glyph List (`glyphlist.txt`,")
    push("//! `zapfdingbats.txt`), licensed under BSD-3-Clause:")
    push("//!")
    push(comment_block(BSD3, "//! > "))
    push("//!")
    push("//! Both licenses also require entries in `THIRD_PARTY_LICENSES.md`")
    push("//! (manual supplementary sections — `cargo-about` cannot see data")
    push("//! dependencies that are not Cargo crates; see `docs/LEGAL.md` §6.3).")
    push("")
    push("use super::{Std14, Std14Descriptor};")
    push("")

    # --- Latin widths ---
    push("/// One row of the shared Latin standard-14 width table: the glyph")
    push("/// name plus the advance (AFM `WX`, glyph space 1000/em) in each of")
    push("/// the six distinct Latin designs. The oblique faces are folded onto")
    push("/// their uprights in `mod.rs` (identical advances, verified); every")
    push("/// Courier face is the constant 600 for any name in this table.")
    push("pub(crate) struct LatinWidths {")
    push("    pub name: &'static str,")
    push("    pub helvetica: u16,")
    push("    pub helvetica_bold: u16,")
    push("    pub times_roman: u16,")
    push("    pub times_bold: u16,")
    push("    pub times_italic: u16,")
    push("    pub times_bold_italic: u16,")
    push("}")
    push("")
    push(f"/// The shared {len(latin_names)}-name Latin repertoire (Helvetica ==")
    push("/// Times == Courier name sets, verified), sorted by name for binary")
    push("/// search. 229 of these are the Adobe standard Latin set of Annex")
    push("/// D.2; the rest are reachable only via `/Differences`.")
    push("#[rustfmt::skip]")
    push("pub(crate) static LATIN_WIDTHS: &[LatinWidths] = &[")
    for name in latin_names:
        r = latin[name]
        push(
            f'    LatinWidths {{ name: "{name}", '
            f"helvetica: {r['helv']}, helvetica_bold: {r['helv_bd']}, "
            f"times_roman: {r['times_r']}, times_bold: {r['times_b']}, "
            f"times_italic: {r['times_i']}, times_bold_italic: {r['times_bi']} }},"
        )
    push("];")
    push("")

    # --- symbolic widths ---
    for const, rows, font in (
        ("SYMBOL_WIDTHS", symbol_sorted, "Symbol"),
        ("ZAPF_DINGBATS_WIDTHS", zapf_sorted, "ZapfDingbats"),
    ):
        push(f"/// `{font}` per-glyph advances ({len(rows)} encoded glyphs), sorted by")
        push("/// glyph name for binary search. AFM `WX`, glyph space 1000/em.")
        push("#[rustfmt::skip]")
        push(f"pub(crate) static {const}: &[(&str, u16)] = &[")
        for r in rows:
            push(f'    ("{r["name"]}", {r["width"]}),')
        push("];")
        push("")

    # --- encodings ---
    win_assigned = sum(1 for e in data["win_enc"] if e is not None)
    bullets = ", ".join(f"{c:#04x}" for c in data["bullet_fill"])
    enc_meta = [
        (
            "STANDARD_ENCODING", data["std_enc"],
            ["/// Annex D.2 `StandardEncoding` (149 assigned codes). Not a legal",
             "/// `/Encoding` name in a file, but required data: the implicit base",
             "/// encoding of non-embedded nonsymbolic fonts (§9.6.6.1)."],
        ),
        (
            "MAC_ROMAN_ENCODING", data["mac_enc"],
            ["/// Annex D.2 `MacRomanEncoding` (207 table codes + the footnote-6",
             "/// nonbreaking space at 0o312). Code 0o333 remains `currency`, not",
             "/// `Euro` — PDF never adopted Apple's reassignment (footnote 1)."],
        ),
        (
            "WIN_ANSI_ENCODING", data["win_enc"],
            [f"/// Annex D.2 `WinAnsiEncoding` ({win_assigned} assigned codes: 216 table",
             "/// codes, the footnote-5/6 soft hyphen 0o255 and nonbreaking space",
             "/// 0o240, and — footnote 3 — every remaining unused code ≥ 0o40",
             f"/// mapped to `bullet`: {bullets}.",
             "/// This is exactly where PDF's WinAnsi differs from CP1252."],
        ),
        (
            "SYMBOL_ENCODING", data["symbol_enc"],
            ["/// `Symbol`'s built-in `FontSpecific` encoding (189 assigned codes)",
             "/// from the font's own AFM `C` codes (§9.6.6.1; spec Annex D.5 —",
             "/// not yet cross-checked against the annex, per the RAG's GAP note).",
             "/// The 190th glyph `apple` is unencoded (AFM `C -1`)."],
        ),
        (
            "ZAPF_DINGBATS_ENCODING", data["zapf_enc"],
            ["/// `ZapfDingbats`' built-in `FontSpecific` encoding (202 assigned",
             "/// codes; spec Annex D.6 — same GAP note as Symbol). The `aNN`",
             "/// glyph-name number is NOT the character code (code 97 is `a60`)."],
        ),
    ]
    for const, enc, doc in enc_meta:
        lines.extend(doc)
        push("#[rustfmt::skip]")
        push(f"pub(crate) static {const}: [Option<&'static str>; 256] = [")
        lines.extend(enc_lines(enc))
        push("];")
        push("")

    # --- AGL subset ---
    push(f"/// Glyph name → Unicode scalar ({len(agl_sorted)} entries), sorted by name")
    push("/// for binary search: the 315-name Latin repertoire (`font__agl.md`),")
    push("/// the 189 encoded `Symbol` names, and the 202 `ZapfDingbats` names")
    push("/// (merged — duplicates verified identical by the generator). All are")
    push("/// single code points; PUA values (U+F6xx/U+F8xx legacy names) are")
    push("/// kept as-is — the extraction layer decides how to surface them.")
    push("#[rustfmt::skip]")
    push("pub(crate) static GLYPH_TO_UNICODE: &[(&str, char)] = &[")
    for name, cp in agl_sorted:
        push(f'    ("{name}", \'\\u{{{cp:04X}}}\'),')
    push("];")
    push("")

    # --- descriptors ---
    push("/// The §9.8.2 Table 122 payload per font. Sourced from the AFM")
    push("/// headers except: `flags` (derived per Table 123) and the two")
    push("/// symbolic fonts' `ascender`/`descender` (bbox-derived) and")
    push("/// `cap_height`/`x_height` (0 — keys absent from their AFMs). See")
    push("/// `font__std14_descriptors.md` for every judgement call.")
    push("#[rustfmt::skip]")
    push("pub(crate) const fn descriptor(font: Std14) -> Std14Descriptor {")
    push("    match font {")
    for base_font, variant in VARIANTS.items():
        d = descriptors[base_font]
        bbox = ", ".join(str(v) for v in d["bbox"])
        italic = f"{d['italic']:.1f}" if d["italic"] != int(d["italic"]) else f"{d['italic']:.1f}"
        push(
            f"        Std14::{variant} => Std14Descriptor {{ "
            f"font_bbox: [{bbox}], "
            f"ascender: {d['asc']}, descender: {d['desc']}, "
            f"cap_height: {d['cap']}, x_height: {d['x']}, "
            f"italic_angle: {italic}, stem_v: {d['stem_v']}, flags: {d['flags']} }},"
        )
    push("    }")
    push("}")
    return "\n".join(lines) + "\n"


def main() -> None:
    if not RAG.is_dir():
        fail(f"RAG not found at {RAG}")
    data = build()
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(emit(data), encoding="utf-8", newline="\n")

    def assigned(enc):
        return sum(1 for e in enc if e is not None)

    print(f"wrote {OUT}")
    print(f"  LATIN_WIDTHS rows:        {len(data['latin'])}")
    print(f"  SYMBOL_WIDTHS rows:       {len(data['symbol'])}")
    print(f"  ZAPF_DINGBATS_WIDTHS rows:{len(data['zapf'])}")
    print(f"  STANDARD_ENCODING codes:  {assigned(data['std_enc'])}")
    print(f"  MAC_ROMAN_ENCODING codes: {assigned(data['mac_enc'])}")
    print(f"  WIN_ANSI_ENCODING codes:  {assigned(data['win_enc'])} "
          f"(incl. {len(data['bullet_fill'])} bullet-fallback)")
    print(f"  SYMBOL_ENCODING codes:    {assigned(data['symbol_enc'])}")
    print(f"  ZAPF_DINGBATS_ENC codes:  {assigned(data['zapf_enc'])}")
    print(f"  GLYPH_TO_UNICODE entries: {len(data['glyph_to_unicode'])}")
    print(f"  descriptors:              {len(data['descriptors'])}")


if __name__ == "__main__":
    main()
