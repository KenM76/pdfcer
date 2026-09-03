#!/usr/bin/env python3
"""Generate the synthetic fixtures Pass 14.1 (in-place text edit) tests
against.

WHY THIS EXISTS
---------------
`docs/LEGAL.md` §5 permits only synthetic or rights-cleared PDFs in
`fixtures/`. Every structural byte of these files is constructed here, from
nothing, by a deliberately minimal writer with no PDF library behind it
(same discipline as the sibling `tools/gen-*-fixtures.py`). Classic
cross-reference table, exactly-20-byte entries (ISO 32000-1 §7.5.4), no
`/ID`, no timestamps -> running this twice produces byte-identical files.

The two fixtures that need an *embedded* font program embed a bundled Foxit
Base-14 face (`crates/pdfcer-render/assets/fonts/FoxitSans.cff`, provenance in
that folder's `PROVENANCE.md`, rights-cleared for redistribution) as a
`/FontFile3 /Subtype /Type1C` program. That keeps the whole corpus
synthetic/rights-cleared while giving Pass 14.1 a genuinely embedded (full)
and a genuinely embedded-subset run to edit.

THE FIXTURES
------------
``nonembedded.pdf``
    A NON-embedded `/Calibri` simple font (`/WinAnsiEncoding`, explicit
    `/Widths`, `/FontDescriptor` with NO `/FontFile*`). One run
    "teh quick brown fox" with a "teh" typo. Proves: the most-editable case
    (no subset limit); the Bundled-vs-Supplied trust disclosure (Bundled with
    no `--font-dir`, Supplied when a `Calibri` face is supplied).

``embedded_full.pdf``
    A `/Helvetica` simple font with an EMBEDDED full program
    (`/FontFile3 /Type1C`, no subset tag). One run "teh cat". Proves: editing
    within an embedded-full program's coverage; `glyph_source=Embedded`;
    renders correctly after the edit; only the edited content stream changes.

``subset_missing.pdf``
    A `/ABCDEF+Helvetica` simple font, EMBEDDED SUBSET (`/FontFile3`, subset
    tag). One run "the cat" -> the page carries only {t,h,e,space,c,a}. Editing
    "cat"->"caz" asks for 'z', which the subset does not already carry ->
    REFUSED by name (the one refusal in decision 014's four-case table).

``tagged.pdf``
    A Tagged-PDF run inside `/P << /MCID 0 >> BDC ... EMC`
    (catalog `/MarkInfo << /Marked true >>`). One run "teh". Editing
    "teh"->"the" must PRESERVE the BDC/EMC+MCID wrapper and DISCLOSE that the
    structure tree's /ActualText / reading order went stale (R72).

``tm_follower.pdf``
    Two runs on one line, the second re-anchored by an absolute `Tm`:
    "Hello " (Td) then "World" (Tm e=240). Editing "Hello"->"Hi" shortens the
    run, so the follower `Tm`'s `e` operand must be reduced by |ΔA|
    (surgery ref §3, the absolute-Tm follower case).

USAGE
-----
    python tools/gen-textedit-fixtures.py

Re-run after changing anything here; the output is committed.
"""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT_DIR = ROOT / "fixtures" / "synthetic" / "textedit"
CFF_PATH = ROOT / "crates" / "pdfcer-render" / "assets" / "fonts" / "FoxitSans.cff"

PAGE_W = 612
PAGE_H = 792

# A uniform /Widths table (WinAnsi codes 32..126). Synthetic: a consistent
# width is all the advance-delta math and the render positions need.
FIRST_CHAR = 32
LAST_CHAR = 126
WIDTHS = "[" + " ".join("500" for _ in range(FIRST_CHAR, LAST_CHAR + 1)) + "]"


def serialize(objects: dict[int, bytes]) -> bytes:
    """Lay `objects` out into a complete file with a classic xref table.

    §7.5.4's entry is exactly 20 bytes. No `/ID`, no timestamps -> byte-stable.
    """
    out = bytearray(b"%PDF-1.7\n")
    out += b"%\xe2\xe3\xcf\xd3\n"  # §7.5.2 binary marker
    highest = max(objects)
    offsets: dict[int, int] = {}
    for num in range(1, highest + 1):
        body = objects.get(num)
        if body is None:
            continue
        offsets[num] = len(out)
        out += f"{num} 0 obj\n".encode("ascii")
        out += body
        out += b"\nendobj\n"
    xref_at = len(out)
    out += f"xref\n0 {highest + 1}\n".encode("ascii")
    out += b"0000000000 65535 f \n"
    for num in range(1, highest + 1):
        if num in offsets:
            out += f"{offsets[num]:010d} 00000 n \n".encode("ascii")
        else:
            out += b"0000000000 65535 f \n"
    out += (
        f"trailer\n<< /Size {highest + 1} /Root 1 0 R >>\n"
        f"startxref\n{xref_at}\n%%EOF\n"
    ).encode("ascii")
    return bytes(out)


def stream(body: bytes, extra: str = "") -> bytes:
    """An uncompressed stream object with a correct `/Length`."""
    return (
        f"<< /Length {len(body)}{extra} >>\nstream\n".encode("ascii")
        + body
        + b"\nendstream"
    )


def simple_page(content: bytes, font_obj: bytes, extra_font_objs: dict[int, bytes],
                catalog_extra: str = "") -> bytes:
    """A one-page document: catalog(1) pages(2) page(3) content(4) font(5)
    plus any extra font objects (>=6)."""
    objects: dict[int, bytes] = {
        1: f"<< /Type /Catalog /Pages 2 0 R{catalog_extra} >>".encode("ascii"),
        2: (
            f"<< /Type /Pages /Kids [3 0 R] /Count 1 "
            f"/MediaBox [0 0 {PAGE_W} {PAGE_H}] "
            f"/Resources << /Font << /F1 5 0 R >> >> >>"
        ).encode("ascii"),
        3: b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>",
        4: stream(content),
        5: font_obj,
    }
    objects.update(extra_font_objs)
    return serialize(objects)


def nonembedded() -> bytes:
    content = b"BT /F1 12 Tf 72 700 Td (teh quick brown fox) Tj ET\n"
    font = (
        f"<< /Type /Font /Subtype /Type1 /BaseFont /Calibri "
        f"/Encoding /WinAnsiEncoding /FirstChar {FIRST_CHAR} /LastChar {LAST_CHAR} "
        f"/Widths {WIDTHS} /FontDescriptor 6 0 R >>"
    ).encode("ascii")
    descriptor = (
        b"<< /Type /FontDescriptor /FontName /Calibri /Flags 32 "
        b"/FontBBox [0 -200 1000 900] /ItalicAngle 0 /Ascent 700 /Descent -200 "
        b"/CapHeight 700 /StemV 80 >>"
    )
    return simple_page(content, font, {6: descriptor})


def embedded(base_font: str, cff: bytes) -> bytes:
    font = (
        f"<< /Type /Font /Subtype /Type1 /BaseFont /{base_font} "
        f"/Encoding /WinAnsiEncoding /FirstChar {FIRST_CHAR} /LastChar {LAST_CHAR} "
        f"/Widths {WIDTHS} /FontDescriptor 6 0 R >>"
    ).encode("ascii")
    descriptor = (
        f"<< /Type /FontDescriptor /FontName /{base_font} /Flags 32 "
        f"/FontBBox [0 -200 1000 900] /ItalicAngle 0 /Ascent 700 /Descent -200 "
        f"/CapHeight 700 /StemV 80 /FontFile3 7 0 R >>"
    ).encode("ascii")
    program = stream(cff, " /Subtype /Type1C")
    return simple_page(EMBED_CONTENT, font, {6: descriptor, 7: program})


EMBED_CONTENT = b"BT /F1 12 Tf 72 700 Td (teh cat) Tj ET\n"
SUBSET_CONTENT = b"BT /F1 12 Tf 72 700 Td (the cat) Tj ET\n"


def embedded_subset(cff: bytes) -> bytes:
    base_font = "ABCDEF+Helvetica"
    font = (
        f"<< /Type /Font /Subtype /Type1 /BaseFont /{base_font} "
        f"/Encoding /WinAnsiEncoding /FirstChar {FIRST_CHAR} /LastChar {LAST_CHAR} "
        f"/Widths {WIDTHS} /FontDescriptor 6 0 R >>"
    ).encode("ascii")
    descriptor = (
        f"<< /Type /FontDescriptor /FontName /{base_font} /Flags 32 "
        f"/FontBBox [0 -200 1000 900] /ItalicAngle 0 /Ascent 700 /Descent -200 "
        f"/CapHeight 700 /StemV 80 /FontFile3 7 0 R >>"
    ).encode("ascii")
    program = stream(cff, " /Subtype /Type1C")
    return simple_page(SUBSET_CONTENT, font, {6: descriptor, 7: program})


def tagged() -> bytes:
    content = (
        b"/P << /MCID 0 >> BDC\n"
        b"BT /F1 12 Tf 72 700 Td (teh) Tj ET\n"
        b"EMC\n"
    )
    font = (
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
        b"/Encoding /WinAnsiEncoding >>"
    )
    return simple_page(content, font, {}, catalog_extra=" /MarkInfo << /Marked true >>")


def tm_follower() -> bytes:
    content = (
        b"BT /F1 12 Tf 100 700 Td (Hello ) Tj "
        b"1 0 0 1 240 700 Tm (World) Tj ET\n"
    )
    font = (
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
        b"/Encoding /WinAnsiEncoding >>"
    )
    return simple_page(content, font, {})


# ---------------------------------------------------------------------------
# Pass 14.2 formatting fixtures (size / fill colour / font-family-style)
# ---------------------------------------------------------------------------
#
# These drive `pdfcer_core::text_edit::format::set_format` (the three formatting
# operations). Same byte-authored, no-library discipline as above.


def custom_page(content: bytes, resources_body: str, extra_objs: dict[int, bytes]) -> bytes:
    """A one-page document whose Resources are written on the PAGE object, so
    a fixture can declare multiple fonts and/or a /ColorSpace. Objects: catalog
    (1) pages(2) page(3) content(4) then the caller's extras (>=5)."""
    objects: dict[int, bytes] = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: (
            f"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 {PAGE_W} {PAGE_H}] >>"
        ).encode("ascii"),
        3: (
            f"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources {resources_body} >>"
        ).encode("ascii"),
        4: stream(content),
    }
    objects.update(extra_objs)
    return serialize(objects)


def simple_font(base_font: str, encoding: str = "/WinAnsiEncoding") -> bytes:
    """A NON-embedded simple Type1 font with an explicit uniform /Widths table
    and the given /BaseFont + /Encoding (a name, or an inline dict)."""
    return (
        f"<< /Type /Font /Subtype /Type1 /BaseFont /{base_font} "
        f"/Encoding {encoding} /FirstChar {FIRST_CHAR} /LastChar {LAST_CHAR} "
        f"/Widths {WIDTHS} >>"
    ).encode("ascii")


def format_color() -> bytes:
    """A NON-embedded /Calibri run "hello world" painted BLUE (`0 0 1 rg`).
    Proves: a size change touches only the Tf operand (blue survives); a colour
    change stores the chosen device space (rg/g/k), never force-DeviceRGB."""
    content = b"BT /F1 12 Tf 0 0 1 rg 72 700 Td (hello world) Tj ET\n"
    font = (
        f"<< /Type /Font /Subtype /Type1 /BaseFont /Calibri "
        f"/Encoding /WinAnsiEncoding /FirstChar {FIRST_CHAR} /LastChar {LAST_CHAR} "
        f"/Widths {WIDTHS} /FontDescriptor 6 0 R >>"
    ).encode("ascii")
    descriptor = (
        b"<< /Type /FontDescriptor /FontName /Calibri /Flags 32 "
        b"/FontBBox [0 -200 1000 900] /ItalicAngle 0 /Ascent 700 /Descent -200 "
        b"/CapHeight 700 /StemV 80 >>"
    )
    resources = "<< /Font << /F1 5 0 R >> >>"
    return custom_page(content, resources, {5: font, 6: descriptor})


def format_other() -> bytes:
    """A run "hello world" whose fill colour is set in a NON-DEVICE space —
    a `/Separation` colour space `/CS0` (`/CS0 cs 0.7 scn`). pdfcer records this
    as `TextColor::Other`. Proves: a colour change on an Other original is
    DISCLOSED as a space-narrowing conversion (never a silent downgrade), and
    the run's tail is restored to the original `scn` sequence byte-verbatim."""
    content = b"/CS0 cs 0.7 scn BT /F1 12 Tf 72 700 Td (hello world) Tj ET\n"
    font = (
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica "
        b"/Encoding /WinAnsiEncoding >>"
    )
    # A valid, minimal 1-in/1-out Separation: tint 0..1 -> DeviceGray 1..0
    # (a Type 2 exponential tint transform), so the page renders.
    tint_fn = b"<< /FunctionType 2 /Domain [0 1] /C0 [1] /C1 [0] /N 1 >>"
    resources = (
        "<< /Font << /F1 5 0 R >> "
        "/ColorSpace << /CS0 [/Separation /Spot1 /DeviceGray 6 0 R] >> >>"
    )
    return custom_page(content, resources, {5: font, 6: tint_fn})


def format_family() -> bytes:
    """A /Times-Roman run "hello world" on a page that ALSO carries two other
    font RESOURCES: /F2 (/Calibri-Bold, a fully-covering target — Bundled, or
    Supplied with `--font-dir`) and /F3 (/Times-Bold whose /Encoding remaps
    code 111 'o' to /bullet, so it does NOT cover the run). Proves: a
    family/style change to a covering target succeeds and re-encodes; a target
    that cannot cover every character is REFUSED with nothing applied."""
    content = b"BT /F1 12 Tf 72 700 Td (hello world) Tj ET\n"
    f1 = simple_font("Times-Roman")
    f2 = simple_font("Calibri-Bold")
    # F3: WinAnsi base, but /Differences steals code 111 ('o') for /bullet, so
    # 'o' has no code in F3 -> a coverage failure on "hello world".
    f3 = simple_font(
        "Times-Bold",
        encoding="<< /Type /Encoding /BaseEncoding /WinAnsiEncoding "
        "/Differences [111 /bullet] >>",
    )
    resources = "<< /Font << /F1 5 0 R /F2 6 0 R /F3 7 0 R >> >>"
    return custom_page(content, resources, {5: f1, 6: f2, 7: f3})


def format_twins() -> bytes:
    """A /Times-Roman run "hello world" on a page carrying TWO resources with
    the SAME /BaseFont /Times-Bold — the shape a real embedding producer emits
    routinely (two independent subsets of one face) and the shape that breaks
    a font list keyed on the name instead of on the resource.

    /FB1 remaps code 111 'o' to /bullet, so it does NOT cover the run.
    /FB2 is the same /BaseFont with a plain WinAnsi encoding and DOES cover it.

    Proves three things nothing else in this directory can:
      * a /BaseFont is not a usable `--set-font` selector when the page
        carries two of them, so the pre-flight reports the resource key
        instead and flags the ambiguity;
      * acceptance is a property of the RESOURCE, not of the name: two
        resources with identical /BaseFont give opposite answers;
      * the synthesis gate must name /FB2, because /FB1 refuses.
    """
    content = b"BT /F1 12 Tf 72 700 Td (hello world) Tj ET\n"
    f1 = simple_font("Times-Roman")
    fb1 = simple_font(
        "Times-Bold",
        encoding="<< /Type /Encoding /BaseEncoding /WinAnsiEncoding "
        "/Differences [111 /bullet] >>",
    )
    fb2 = simple_font("Times-Bold")
    resources = "<< /Font << /F1 5 0 R /FB1 6 0 R /FB2 7 0 R >> >>"
    return custom_page(content, resources, {5: f1, 6: fb1, 7: fb2})


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    cff = CFF_PATH.read_bytes()
    fixtures = {
        "nonembedded.pdf": nonembedded(),
        "embedded_full.pdf": embedded("Helvetica", cff),
        "subset_missing.pdf": embedded_subset(cff),
        "tagged.pdf": tagged(),
        "tm_follower.pdf": tm_follower(),
        "format_color.pdf": format_color(),
        "format_other.pdf": format_other(),
        "format_family.pdf": format_family(),
        "format_twins.pdf": format_twins(),
    }
    for name, data in fixtures.items():
        path = OUT_DIR / name
        path.write_bytes(data)
        print(f"wrote {path} - {len(data)} bytes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
