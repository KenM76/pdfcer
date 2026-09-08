#!/usr/bin/env python3
"""Generate the synthetic SYMBOLIC-TrueType glyph-selection RENDER fixture.

# What this reproduces, and why it needed its own fixture

ISO 32000-1 §9.6.6.4 splits simple TrueType glyph selection in two:

* **Branch A** (nonsymbolic, `/Encoding` present) — code -> glyph NAME, then
  name -> Unicode -> `(3,1)` cmap; failing that, name -> a Mac OS Roman code
  -> `(1,0)` cmap; failing that, the `post` table.
* **Branch B** (`Symbolic` flag set, in which case *"the `Encoding` entry is
  ignored"*) — the raw code, straight into the program's own cmap.

pdfce used to run Branch A first whenever a glyph name was present, symbolic
or not. That is wrong for a symbolic font, and it fails in the worst possible
way: **Branch A's second chain does not MISS on a symbolic subset, it HITS the
wrong glyph.**

The reason is that platform 1 / encoding 0 *means* Mac OS Roman, and Branch A
is entitled to assume it. A subsetter writing a symbolic font does not honour
that: it emits a private `(1,0)` table whose codes are 1, 2, 3 ... in the order
the glyphs happened to be used. Looking a Mac OS Roman code up in that table
returns a perfectly valid GID for a completely unrelated glyph.

Found 2026-09-08 on a real 2013 SolidWorks drawing, where `59 3/4"` painted as
`@ U / M@`-shaped nonsense while **text extraction was correct** -- extraction
reads the `/Differences` names, which were right the whole time. A file that
renders as garbage and copies as clean text is this bug's signature.

# The trap this fixture sets, which is the whole point

A naive fixture -- private codes only -- does NOT catch the regression. Under
the old code Branch A's chains would simply fail (no `(3,1)`, no matching Mac
code, no `post` name), the ladder would fall through to Branch B anyway, and
the fixture would pass against the defect it was written for.

So this font's `(1,0)` cmap is populated **twice over**:

    code 1, 2, 3     -> GID of `boxLow1`, `boxLow2`, `boxLow3`   (CORRECT)
    code 65, 66, 67  -> GID of `boxHigh1`, `boxHigh2`, `boxHigh3` (WRONG)

and the PDF says `/Differences [1 /A /B /C]`. Mac OS Roman codes for `A`, `B`
and `C` are 65, 66 and 67 -- so Branch A chain 2 lands squarely on the second
set. Both outcomes are *valid glyphs that paint ink*; only WHERE they paint
differs, which is what makes the assertion possible.

`boxLow*` paint in the lower half of the em square, `boxHigh*` in the upper
half. So:

    correct  -> ink in the lower band, none in the upper
    buggy    -> ink in the upper band, none in the lower

Ink-vs-no-ink, not glyph identity, so the test does not depend on rasteriser
detail, hinting, or antialiasing thresholds.

There is deliberately **no `(3,1)` subtable**, matching the real file: with one
present, `glyph_for_mac_code` declines by design ("the (3,1) chain owns this
font") and the wrong-glyph path is unreachable.

Usage:  python tools/gen-symbolic-truetype-fixtures.py
Output: fixtures/synthetic/text/symbolic-truetype-private-cmap.pdf
"""

from __future__ import annotations

import io
import pathlib

OUT_DIR = pathlib.Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "text"

UPEM = 1000

# The three codes the content stream actually shows, and the glyph names the
# PDF's /Differences gives them. The names are chosen so that their Mac OS
# Roman codes (65/66/67) collide with real entries in the font's private
# (1,0) table -- see the module docstring.
CODES = [1, 2, 3]
DIFF_NAMES = ["A", "B", "C"]
MAC_CODES = [65, 66, 67]


def build_symbolic_truetype() -> bytes:
    """A symbolic-style TrueType whose `(1,0)` cmap is a PRIVATE encoding.

    Seven glyphs: `.notdef`, three `boxLow*` (the correct targets) and three
    `boxHigh*` (the wrong ones Branch A's Mac chain would reach).
    """
    from fontTools.fontBuilder import FontBuilder
    from fontTools.pens.ttGlyphPen import TTGlyphPen

    low = ["boxLow1", "boxLow2", "boxLow3"]
    high = ["boxHigh1", "boxHigh2", "boxHigh3"]
    glyph_order = [".notdef"] + low + high

    fb = FontBuilder(UPEM, isTTF=True)
    fb.setupGlyphOrder(glyph_order)

    def box(y0: int, y1: int, x0: int, x1: int):
        pen = TTGlyphPen(None)
        pen.moveTo((x0, y0))
        pen.lineTo((x1, y0))
        pen.lineTo((x1, y1))
        pen.lineTo((x0, y1))
        pen.closePath()
        return pen.glyph()

    glyphs = {".notdef": TTGlyphPen(None).glyph()}
    # Lower band: y 0..350 of the em square. Upper band: y 500..850.
    for i, name in enumerate(low):
        glyphs[name] = box(0, 350, 100 + 0 * i, 600)
    for i, name in enumerate(high):
        glyphs[name] = box(500, 850, 100 + 0 * i, 600)

    fb.setupGlyf(glyphs)
    fb.setupHorizontalMetrics({n: (UPEM, 100) for n in glyph_order})
    fb.setupHorizontalHeader(ascent=800, descent=-200)
    fb.setupNameTable({"familyName": "pdfcerSymbolicPrivate", "styleName": "Regular"})

    # fontBuilder wants a character map before OS/2 (it derives Unicode
    # ranges from one). Give it the mapping we actually want, then rewrite
    # the cmap table below into the (1,0)+(3,0) shape a symbolic subset has.
    fb.setupCharacterMap({0x41: "boxLow1"})
    fb.setupOS2(sTypoAscender=800, sTypoDescender=-200)
    fb.setupPost()

    font = fb.font

    # ---- the private cmap, built by hand -------------------------------
    #
    # Two subtables, no (3,1):
    #   (1,0) format 0 -- the private codes AND the colliding Mac codes
    #   (3,0) format 4 -- the same private codes at 0xF001..0xF003
    #
    # `post` is set to version 3.0 (no glyph names) so Branch A's third
    # chain cannot rescue the lookup either. A real subset does the same.
    from fontTools.ttLib.tables._c_m_a_p import CmapSubtable

    mac_sub = CmapSubtable.newSubtable(0)
    mac_sub.platformID, mac_sub.platEncID, mac_sub.language = 1, 0, 0
    mac_sub.cmap = {}
    for code, name in zip(CODES, low):
        mac_sub.cmap[code] = name
    for code, name in zip(MAC_CODES, high):
        mac_sub.cmap[code] = name

    sym_sub = CmapSubtable.newSubtable(4)
    sym_sub.platformID, sym_sub.platEncID, sym_sub.language = 3, 0, 0
    sym_sub.cmap = {0xF000 | code: name for code, name in zip(CODES, low)}

    font["cmap"].tables = [mac_sub, sym_sub]
    font["post"].formatType = 3.0

    buf = io.BytesIO()
    font.save(buf)
    data = buf.getvalue()

    # Verify-don't-assume (R22): re-read the SAVED bytes. A fixture that
    # quietly lost the collision would still render, still look plausible,
    # and would stop testing the thing it is named after.
    import struct

    n = struct.unpack(">H", data[4:6])[0]
    tabs = {}
    for i in range(n):
        off = 12 + 16 * i
        tabs[data[off:off + 4].decode("latin1")] = struct.unpack(">II", data[off + 8:off + 16])
    assert "cmap" in tabs, "no cmap survived"
    co, cl = tabs["cmap"]
    c = data[co:co + cl]
    subs = []
    nt = struct.unpack(">H", c[2:4])[0]
    for i in range(nt):
        pid, eid, so = struct.unpack(">HHI", c[4 + 8 * i:12 + 8 * i])
        subs.append((pid, eid, so))
    have = {(p, e) for p, e, _ in subs}
    assert (1, 0) in have, f"no (1,0) subtable: {have}"
    assert (3, 0) in have, f"no (3,0) subtable: {have}"
    assert (3, 1) not in have, "a (3,1) subtable makes the wrong-glyph path unreachable"

    # The collision itself: the (1,0) table must map BOTH the private codes
    # and the Mac codes, to DIFFERENT glyphs.
    so = next(s for p, e, s in subs if (p, e) == (1, 0))
    gids = c[so + 6:so + 6 + 256]
    for priv, mac in zip(CODES, MAC_CODES):
        assert gids[priv] != 0, f"private code {priv} unmapped"
        assert gids[mac] != 0, f"mac code {mac} unmapped -- the trap is not set"
        assert gids[priv] != gids[mac], (
            f"codes {priv} and {mac} resolve to the same GID; the fixture "
            f"cannot tell a correct render from a buggy one"
        )
    return data


def serialize(objects: dict[int, bytes]) -> bytes:
    """Classic xref layout, exactly-20-byte entries (§7.5.4)."""
    out = bytearray(b"%PDF-1.7\n")
    out += b"%\xe2\xe3\xcf\xd3\n"
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


def raw_stream(body: bytes, extra: str) -> bytes:
    """Uncompressed stream with a correct `/Length`, so a failure can never
    be blamed on a filter."""
    return (
        f"<< /Length {len(body)}{extra} >>\nstream\n".encode("ascii")
        + body
        + b"\nendstream"
    )


def embedded_with_flags(flags: int) -> bytes:
    """One page, one text run showing codes 1, 2 and 3 at 100pt.

    `flags` is the descriptor's `/Flags`: **4** = `Symbolic`, **32** =
    `Nonsymbolic` (Table 123 bits 3 and 6, 1-based). The `/Encoding`
    dictionary is present in BOTH -- for the symbolic one the standard calls
    that a *should not* and real producers emit it constantly, which is
    exactly the combination the renderer used to mishandle.

    # ★ The two files are a MIRROR PAIR over one font program

    Same bytes, same content stream, same `/Differences`; only `/Flags`
    differs. And the correct answer INVERTS:

    | `/Flags` | branch | reaches | band |
    |---|---|---|---|
    | 4 (symbolic) | B -- raw code | `boxLow*` via `(1,0)[1..3]` | **low** |
    | 32 (nonsymbolic) | A -- glyph name | `boxHigh*` via Mac code 65..67 | **high** |

    That inversion is what makes the `Symbolic` test *provable* rather than
    merely *plausible*. A single fixture can only show that one branch works;
    a rule that ignored the flag entirely and always took Branch B would
    satisfy the symbolic file and fail only here. Neither file alone can
    catch that -- which is the point, and is why this generator emits both
    from one font rather than tuning a second font to agree.
    """
    ttf = build_symbolic_truetype()
    # 100pt text at y=100 puts the LOW band around y=100..135 and the HIGH
    # band around y=150..185, in a 300x300 page. Well separated.
    content = b"BT /F1 100 Tf 20 100 Td <010203> Tj ET\n"
    objects = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        3: (
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] "
            b"/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
        ),
        4: raw_stream(content, ""),
        5: (
            b"<< /Type /Font /Subtype /TrueType /BaseFont /AAAAAA+pdfcerSymbolicPrivate "
            b"/FirstChar 1 /LastChar 3 /Widths [1000 1000 1000] "
            b"/Encoding 6 0 R /FontDescriptor 7 0 R >>"
        ),
        6: (
            b"<< /Type /Encoding /BaseEncoding /WinAnsiEncoding "
            b"/Differences [1 /A /B /C] >>"
        ),
        7: (
            b"<< /Type /FontDescriptor /FontName /AAAAAA+pdfcerSymbolicPrivate "
            b"/Flags 4 /FontBBox [0 -200 1000 800] /ItalicAngle 0 /Ascent 800 "
            b"/Descent -200 /CapHeight 700 /StemV 80 /FontFile2 8 0 R >>"
        ).replace(b"/Flags 4 ", f"/Flags {flags} ".encode("ascii")),
        8: raw_stream(ttf, f" /Length1 {len(ttf)}"),
    }
    return serialize(objects)


def not_embedded(flags: int) -> bytes:
    """The same text and `/Differences`, with **NO embedded program**.

    Emitted twice -- once `/Flags 4` (symbolic), once `/Flags 32`
    (nonsymbolic) -- as an A/B pair.

    # Why this pair exists

    The renderer's rule is *"symbolic **AND EMBEDDED** -> the program's own
    cmap first"*, and the `embedded` half needs its own test or it is a
    guard nobody has ever seen fail.

    It is load-bearing: with no embedded program the "program" is a
    SUBSTITUTE face, whose built-in encoding has no relationship whatever to
    this document's codes. Codes 1, 2 and 3 are C0 control positions in any
    normal face, so a raw-code lookup there yields nothing and the text
    vanishes. The `/Differences` names are the only usable route, exactly as
    §9.6.6.4's implicit-base table says (*"no program embedded + symbolic ->
    the font's built-in encoding"* applies to a font program that is
    actually THERE).

    Rendering the pair and requiring the two to agree is the assertion,
    because it needs no knowledge of which substitute face was picked: under
    the correct rule both take the name chains and paint identically; under
    a rule that dropped the `embedded` half, only the nonsymbolic one does.
    """
    content = b"BT /F1 100 Tf 20 100 Td <010203> Tj ET\n"
    objects = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        3: (
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] "
            b"/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
        ),
        4: raw_stream(content, ""),
        5: (
            b"<< /Type /Font /Subtype /TrueType /BaseFont /Helvetica "
            b"/FirstChar 1 /LastChar 3 /Widths [1000 1000 1000] "
            b"/Encoding 6 0 R /FontDescriptor 7 0 R >>"
        ),
        6: (
            b"<< /Type /Encoding /BaseEncoding /WinAnsiEncoding "
            b"/Differences [1 /A /B /C] >>"
        ),
        7: (
            b"<< /Type /FontDescriptor /FontName /Helvetica "
            + f"/Flags {flags} ".encode("ascii")
            + b"/FontBBox [0 -200 1000 800] /ItalicAngle 0 /Ascent 800 "
            b"/Descent -200 /CapHeight 700 /StemV 80 >>"
        ),
    }
    return serialize(objects)


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    written = [
        ("symbolic-truetype-private-cmap.pdf", embedded_with_flags(4)),
        ("nonsymbolic-truetype-private-cmap.pdf", embedded_with_flags(32)),
        ("symbolic-truetype-not-embedded.pdf", not_embedded(4)),
        ("nonsymbolic-truetype-not-embedded.pdf", not_embedded(32)),
    ]
    for name, data in written:
        (OUT_DIR / name).write_bytes(data)
        print(f"wrote {name} ({len(data)} bytes) to {OUT_DIR}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
