#!/usr/bin/env python3
"""Generate the huge-object-number save-refusal fixture.

WHY THIS FIXTURE EXISTS
-----------------------
ISO 32000-1 §7.5.4's completeness requirement says the cross-reference
table "shall contain one entry for each object number from 0 to the
maximum object number defined in the file, even if one or more of the
object numbers in this range do not actually occur in the file". For a
**full rewrite** — one section — that obligation lands entirely on that
section, so pdfcer must emit one entry per number from 0 to the highest.

That makes the writer's cost a function of the largest object NUMBER,
not of how many objects the file contains. A tiny document naming one
enormous object number therefore asks for an enormous table.

This fixture is that shape, minimally: **six real objects, one of which
is numbered 2147483648.** A full rewrite of it would need 2,147,483,649
cross-reference entries — roughly 40 GB — from a document under 1 KB.

Measured on the real-world original before the guard existed: ~27 MB/s
of steady allocation with the CPU pinned, sustained for over thirty
minutes without finishing. Not an infinite loop; worse in one respect,
because it looks like progress the whole way down. In the GUI that is an
unrecoverable freeze — no error, no cancel, no save.

WHY SYNTHETIC RATHER THAN THE CORPUS FILE
------------------------------------------
The behaviour was found on pdfium's ``bug_455199.pdf``, which IS vendored
and rights-cleared. But `fixtures/external/` is populated by
``fixtures/fetch-corpora.sh`` and is **absent on a fresh clone**, so a
regression test bound to it would silently not run — which is precisely
the "green is not evidence" shape the guard was written against. A
synthetic fixture is checked in, always present, and documents its own
adversarial intent (LEGAL §5: self-authored, no third-party content).

The object number is chosen as **2³¹ = 2147483648** deliberately: Annex C
Table C.1 caps a PDF integer at 2,147,483,647, so this is one MORE than
the largest integer the spec permits. The number is not merely
implausible — it is unrepresentable as a conforming PDF integer, which
makes refusing it uncontroversial.

WHAT THE FILE IS
----------------
A valid, loadable one-page document. Everything except the object
numbering is ordinary, so a test that fails on it is failing on the
numbering and nothing else:

    1  Catalog
    2  Pages       (/Count 1, /Kids [3 0 R])
    3  Page        (/Contents 2147483648 0 R)
    4  Font        (Helvetica, standard-14, no embedding)
    2147483648     the content stream — the whole point

The xref is a **two-subsection** classic table (`0 5` then
`2147483648 1`), which §7.5.4 explicitly permits: "one or more
subsections, which may appear in any order". So the file is well-formed
on the READ side, and pdfcer opens it without recovery — the refusal
under test is a *writer* refusal, not a parse failure, and the fixture
would prove nothing if it could not be read cleanly.

Regenerate with ``python tools/gen-huge-objnum-fixture.py``. CC0.
"""

from pathlib import Path

# 2^31 — one more than Annex C Table C.1's maximum integer (2^31 - 1).
HUGE = 2_147_483_648

OUT_DIR = Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "xref-recover"
OUT = OUT_DIR / "huge-object-number.pdf"

CONTENT = b"BT /F1 18 Tf 60 120 Td (huge object number) Tj ET\n"


def build() -> bytes:
    """Assemble the file, recording each object's byte offset as it goes."""
    out = bytearray()
    offsets: dict[int, int] = {}

    def obj(num: int, body: bytes) -> None:
        offsets[num] = len(out)
        out.extend(f"{num} 0 obj\n".encode("ascii"))
        out.extend(body)
        out.extend(b"\nendobj\n")

    out.extend(b"%PDF-1.7\n")
    # Binary comment (§7.5.2) so transfer tools treat the file as binary.
    out.extend(b"%\xe2\xe3\xcf\xd3\n")

    obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
    obj(2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>")
    obj(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200]"
        b" /Resources << /Font << /F1 4 0 R >> >>"
        + f" /Contents {HUGE} 0 R >>".encode("ascii"),
    )
    obj(4, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    obj(
        HUGE,
        f"<< /Length {len(CONTENT)} >>\nstream\n".encode("ascii")
        + CONTENT
        + b"endstream",
    )

    # Two subsections: `0 5` for the ordinary objects, then a second for
    # the outlier. §7.5.4 permits "one or more subsections ... in any
    # order", so this is conforming, not a trick — the file must READ
    # cleanly or the writer-refusal test would be testing a parse error.
    xref_at = len(out)
    out.extend(b"xref\n")
    out.extend(b"0 5\n")
    out.extend(b"0000000000 65535 f \n")
    for num in (1, 2, 3, 4):
        out.extend(f"{offsets[num]:010d} 00000 n \n".encode("ascii"))
    out.extend(f"{HUGE} 1\n".encode("ascii"))
    out.extend(f"{offsets[HUGE]:010d} 00000 n \n".encode("ascii"))

    # /Size is "1 greater than the highest object number" (§7.5.5 Table
    # 15), which for this file is genuinely 2147483649.
    out.extend(
        f"trailer\n<< /Size {HUGE + 1} /Root 1 0 R >>\n"
        f"startxref\n{xref_at}\n%%EOF\n".encode("ascii")
    )
    return bytes(out)


def main() -> None:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    data = build()
    OUT.write_bytes(data)
    print(f"wrote {OUT} ({len(data)} bytes), highest object number {HUGE}")


if __name__ == "__main__":
    main()
