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

Run: python tools/gen-resubset-fixtures.py
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
    for name, data in [("simple.pdf", simple), ("type0-identity.pdf", type0)]:
        (OUT / name).write_bytes(data)
        print(f"wrote {OUT / name} ({len(data)} B)")


if __name__ == "__main__":
    main()
