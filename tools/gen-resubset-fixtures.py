#!/usr/bin/env python3
"""Generate fixtures/synthetic/resubset/: PDFs for `optimize --resubset-fonts`.

Synthetic per docs/LEGAL.md §5: every PDF byte is written here, and the one
embedded program is the synthetic fixtures/synthetic/text/augment/face.ttf
(`.notdef A B C D E acute Eacute`, Eacute = E + acute; from
tools/gen-augment-face-fixtures.py). Classic xref, no /ID, no dates, so a
rerun is byte-identical.

- simple.pdf: TrueType, WinAnsi, shows "A\311" (A and Eacute), so the cut
  keeps .notdef, A, E, acute and Eacute (the composite closure) and empties
  B, C and D.
- type0-identity.pdf: Type0 / Identity-H over CIDFontType2 with a
  /CIDToGIDMap stream (CID 1 -> glyph 3, "C"); shows CID 1, keeping
  .notdef and C.
- pair.pdf: the face held twice, as a Word document holds it: a TrueType
  font (WinAnsi, shows "A") and a Type0 / Identity-H font over a
  CIDFontType2 with /CIDToGIDMap /Identity (shows glyph 3, "C"), each with
  its own descriptor and its own copy of the program, under two subset
  tags. A merge leaves one program holding .notdef, A and C.
- shared-program.pdf: two TrueType fonts, each with its own descriptor,
  both naming one program: F1 shows "A" and F2 shows "\311" (Eacute). The
  one cut keeps the glyphs both draw: .notdef, A, E, acute and Eacute.

- symbolic.pdf: a symbolic TrueType font (/Flags 4, no /Encoding) over
  face.ttf with its cmap replaced by one (3,0) subtable, F041-F045 -> A-E;
  shows "B", so the cut keeps .notdef and B (ISO 32000-1 9.6.6.4).

Run: python tools/gen-resubset-fixtures.py (symbolic.pdf needs fontTools)
"""

from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FACE = (ROOT / "fixtures/synthetic/text/augment/face.ttf").read_bytes()
OUT = ROOT / "fixtures/synthetic/resubset"

PAGE = (b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 140 80] /Contents 4 0 R "
        b"/Resources << /Font << /F1 5 0 R >> >> >>")
DESCRIPTOR = (b"<< /Type /FontDescriptor /FontName /ABCDEF+Face /Flags 32 "
              b"/FontBBox [0 -200 1000 900] /ItalicAngle 0 /Ascent 800 /Descent -200 "
              b"/CapHeight 700 /StemV 80 /FontFile2 7 0 R >>")


def stream(extra: bytes, data: bytes) -> bytes:
    return b"<< %s/Length %d >>\nstream\n" % (extra, len(data)) + data + b"\nendstream"


def assemble(bodies: list[bytes]) -> bytes:
    out = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n"
    at = []
    for i, body in enumerate(bodies):
        at.append(len(out))
        out += b"%d 0 obj\n" % (i + 1) + body + b"\nendobj\n"
    xref, n = len(out), len(bodies) + 1
    out += b"xref\n0 %d\n0000000000 65535 f \n" % n
    out += b"".join(b"%010d 00000 n \n" % o for o in at)
    out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (n, xref)
    return out


def document(content: bytes, font: bytes, extra: list[bytes]) -> bytes:
    return assemble([
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        PAGE,
        stream(b"", content),
        font,
        DESCRIPTOR,
        stream(b"/Length1 %d " % len(FACE), FACE),
        *extra,
    ])


def pair() -> bytes:
    page = (b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 140 80] /Contents 4 0 R "
            b"/Resources << /Font << /F1 5 0 R /F2 8 0 R >> >> >>")
    other = DESCRIPTOR.replace(b"ABCDEF+", b"GHIJKL+").replace(b"7 0 R", b"11 0 R")
    return assemble([
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        page,
        stream(b"", b"BT /F1 48 Tf 10 20 Td (A) Tj /F2 48 Tf 50 0 Td <0003> Tj ET"),
        b"<< /Type /Font /Subtype /TrueType /BaseFont /ABCDEF+Face "
        b"/Encoding /WinAnsiEncoding /FontDescriptor 6 0 R >>",
        DESCRIPTOR,
        stream(b"/Length1 %d " % len(FACE), FACE),
        b"<< /Type /Font /Subtype /Type0 /BaseFont /GHIJKL+Face "
        b"/Encoding /Identity-H /DescendantFonts [9 0 R] >>",
        b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /GHIJKL+Face "
        b"/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> "
        b"/FontDescriptor 10 0 R /CIDToGIDMap /Identity >>",
        other,
        stream(b"/Length1 %d " % len(FACE), FACE),
    ])



def shared_program() -> bytes:
    page = (b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 140 80] /Contents 4 0 R "
            b"/Resources << /Font << /F1 5 0 R /F2 8 0 R >> >> >>")
    font = (b"<< /Type /Font /Subtype /TrueType /BaseFont /ABCDEF+Face "
            b"/Encoding /WinAnsiEncoding /FontDescriptor %d 0 R >>")
    return assemble([
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        page,
        stream(b"", b"BT /F1 48 Tf 10 20 Td (A) Tj /F2 48 Tf 50 0 Td (\311) Tj ET"),
        font % 6,
        DESCRIPTOR,
        stream(b"/Length1 %d " % len(FACE), FACE),
        font % 9,
        DESCRIPTOR,
    ])


def symbolic_face() -> bytes:
    """face.ttf with only a (3,0) cmap; timestamps kept, so a rerun is identical."""
    import io

    from fontTools.ttLib import TTFont, newTable
    from fontTools.ttLib.tables._c_m_a_p import cmap_format_4

    font = TTFont(io.BytesIO(FACE), recalcTimestamp=False)
    sub = cmap_format_4(4)
    sub.platformID, sub.platEncID, sub.language = 3, 0, 0
    sub.cmap = {0xF041 + i: name for i, name in enumerate("ABCDE")}
    table = newTable("cmap")
    table.tableVersion, table.tables = 0, [sub]
    font["cmap"] = table
    buf = io.BytesIO()
    font.save(buf, reorderTables=True)
    return buf.getvalue()


def symbolic() -> bytes:
    program = symbolic_face()
    return assemble([
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        PAGE,
        stream(b"", b"BT /F1 48 Tf 10 20 Td (B) Tj ET"),
        b"<< /Type /Font /Subtype /TrueType /BaseFont /ABCDEF+Face "
        b"/FontDescriptor 6 0 R >>",
        DESCRIPTOR.replace(b"/Flags 32", b"/Flags 4"),
        stream(b"/Length1 %d " % len(program), program),
    ])

def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    simple = document(
        b"BT /F1 48 Tf 10 20 Td (A\311) Tj ET",
        b"<< /Type /Font /Subtype /TrueType /BaseFont /ABCDEF+Face "
        b"/Encoding /WinAnsiEncoding /FontDescriptor 6 0 R >>",
        [],
    )
    cid_map = b"\x00\x00\x00\x03"
    type0 = document(
        b"BT /F1 48 Tf 10 20 Td <0001> Tj ET",
        b"<< /Type /Font /Subtype /Type0 /BaseFont /ABCDEF+Face "
        b"/Encoding /Identity-H /DescendantFonts [8 0 R] >>",
        [
            b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /ABCDEF+Face "
            b"/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> "
            b"/FontDescriptor 6 0 R /CIDToGIDMap 9 0 R >>",
            stream(b"", cid_map),
        ],
    )
    for name, data in [
        ("simple.pdf", simple),
        ("type0-identity.pdf", type0),
        ("pair.pdf", pair()),
        ("shared-program.pdf", shared_program()),
        ("symbolic.pdf", symbolic()),
    ]:
        (OUT / name).write_bytes(data)
        print(f"wrote {OUT / name} ({len(data)} B)")


if __name__ == "__main__":
    main()
