#!/usr/bin/env python3
"""Generate synthetic classic-`xref` EOL fixtures (ISO 32000-1 §7.5.4).

Pass A of pdfcer decision 013 (xref recovery) is a MEASUREMENT step whose
first deliverable is to PROVE pdfcer-core's classic cross-reference-table
parser is correct across every end-of-line form §7.5.4 and §7.5.1 permit —
so that the real-world CRLF-correlated load failures can be attributed to
offset-shift corruption (Pass B territory) rather than to a phantom parser
bug. These fixtures are the positive controls: every one is a WELL-FORMED
single-revision PDF with a valid classic table, and every one MUST load.

## What §7.5.4 fixes and what it leaves free

The 20-byte entry is the ONLY byte-exact part of a classic table. Each entry
is `nnnnnnnnnn SP ggggg SP t EOL` where the 2-byte `EOL` is exactly one of:

    SP CR   (20 0D)      SP LF   (20 0A)      CR LF   (0D 0A)

Everything else on the section's *structural* lines — the `xref` keyword
line, each `first count` subsection header, the `trailer` keyword, the
`startxref` line — falls back to §7.5.1's general line rule: a line may end
in CR, LF, or CR LF, and (§7.5.4 header text) the two subsection numbers are
separated by a single SPACE with no fixed width or padding. pdfcer's parser
reads those structural lines through the whitespace-skipping lexer, so any
CR/LF/CRLF and incidental trailing spaces are legal there.

## The fixture matrix (all well-formed, all must load)

| file                     | entry EOL | structural EOL | shape                         |
|--------------------------|-----------|----------------|-------------------------------|
| entry-spcr.pdf           | SP CR     | LF             | every entry ends `20 0D`      |
| entry-splf.pdf           | SP LF     | LF             | the common form               |
| entry-crlf.pdf           | CR LF     | LF             | every entry ends `0D 0A`      |
| struct-cr.pdf            | SP LF     | CR             | §7.5.1 bare-CR structural EOL |
| struct-crlf.pdf          | CR LF     | CR LF          | CRLF on every line            |
| multi-subsection.pdf     | SP LF     | LF             | 3 subsections, non-contiguous |
| trailing-space.pdf       | SP LF     | LF             | trailing SP before header EOL |
| bare-cr-oldmac.pdf       | SP CR     | CR             | old-Mac: every EOL is CR      |
| mixed-eol.pdf            | rotates   | rotates        | entries + lines mix all forms |

`entry-*` prove the three legal entry EOLs; `struct-*`, `trailing-space`,
`bare-cr-oldmac` prove structural-line EOL tolerance; `multi-subsection`
proves subsection-relative addressing under a non-`0 N` layout; `mixed-eol`
proves the parser never assumes one uniform EOL within a file.

Every fixture is a complete, resolvable document (catalog -> pages -> one
page), so `Document::from_bytes` succeeds end to end, not merely the xref
layer. Object offsets are computed against the ACTUAL assembled bytes.

Synthetic, self-authored — LEGAL §5 compliant (no third-party content).
Regenerate with `python tools/gen-xref-eol-fixtures.py`.
"""
import os

OUT = "fixtures/synthetic/xref-eol"

# The three legal 2-byte entry EOLs (§7.5.4).
SP_CR = b" \r"
SP_LF = b" \n"
CR_LF = b"\r\n"


def objects():
    """The four body objects of a minimal one-page document.

    Object 1 is the catalog (so `/Root 1 0 R`), 2 the page tree, 3 the page,
    4 the page's content stream. Bodies use LF internally; that is
    irrelevant to the xref-EOL question under test.
    """
    content = b"BT /F1 12 Tf 72 720 Td (xref EOL fixture) Tj ET"
    return {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        3: (
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            b"/Resources << /Font << /F1 << /Type /Font /Subtype /Type1 "
            b"/BaseFont /Helvetica >> >> >> /Contents 4 0 R >>"
        ),
        4: b"<< /Length %d >>\nstream\n%s\nendstream" % (len(content), content),
    }


def build(entry_eols, struct_eols, subsections=None, trailing_space=False):
    """Assemble a complete PDF with fully-controlled xref EOLs.

    `entry_eols`  — list cycled per entry, each one of SP_CR/SP_LF/CR_LF.
    `struct_eols` — list cycled per structural line, each CR/LF/CRLF.
    `subsections` — list of `(first, count)` covering objects
                    `first..first+count-1`; `None` means one `0 <size>`
                    subsection. Their union must be exactly `0..=size-1`.
    `trailing_space` — append a SPACE before the EOL of the `xref` keyword
                       line and each subsection header (skip_one_eol / lexer
                       must tolerate it).
    """
    objs = objects()
    size = max(objs) + 1

    # --- body: header + objects, recording each object's byte offset ---
    buf = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n"
    off = {}
    for n in sorted(objs):
        off[n] = len(buf)
        buf += b"%d 0 obj\n" % n + objs[n] + b"\nendobj\n"

    xref_at = len(buf)

    # structural-EOL cycler
    si = [0]

    def seol():
        e = struct_eols[si[0] % len(struct_eols)]
        si[0] += 1
        return (b" " if trailing_space else b"") + e

    # entry-EOL cycler
    ei = [0]

    def entry(field1, gen, kind):
        e = entry_eols[ei[0] % len(entry_eols)]
        ei[0] += 1
        rec = b"%010d %05d %c" % (field1, gen, kind) + e
        assert len(rec) == 20, (len(rec), rec)  # §7.5.4 exactly-20-byte guard
        return rec

    if subsections is None:
        subsections = [(0, size)]

    # Every object 0..size-1 must be covered exactly once (§7.5.4 union
    # completeness); assert it so a bad matrix fails loudly at generation.
    covered = sorted(n for first, count in subsections for n in range(first, first + count))
    assert covered == list(range(size)), (covered, size)

    # --- xref section ---
    x = b"xref" + seol()
    for first, count in subsections:
        x += b"%d %d" % (first, count) + seol()
        for num in range(first, first + count):
            if num == 0:
                x += entry(0, 65535, ord("f"))  # free-list head
            else:
                x += entry(off[num], 0, ord("n"))

    # --- trailer + startxref ---
    x += b"trailer" + seol()
    x += b"<< /Size %d /Root 1 0 R >>" % size + seol()
    x += b"startxref" + seol()
    x += b"%d" % xref_at + seol()
    x += b"%%EOF" + seol()

    return buf + x


def main():
    os.makedirs(OUT, exist_ok=True)
    LF = [b"\n"]
    CR = [b"\r"]
    CRLF = [b"\r\n"]

    fixtures = {
        # Three legal entry EOLs, LF structural lines.
        "entry-spcr.pdf": build([SP_CR], LF),
        "entry-splf.pdf": build([SP_LF], LF),
        "entry-crlf.pdf": build([CR_LF], LF),
        # Structural-line EOL tolerance.
        "struct-cr.pdf": build([SP_LF], CR),
        "struct-crlf.pdf": build([CR_LF], CRLF),
        # Non-contiguous multi-subsection: objects 1..4 split into
        # `0 1`(obj0 free) + `1 1` + `3 2`, out of natural order, plus a
        # gap the union still covers because every number 0..4 appears.
        "multi-subsection.pdf": build(
            [SP_LF], LF,
            subsections=[(0, 1), (3, 2), (1, 2)],
        ),
        # Trailing space before each structural-line EOL.
        "trailing-space.pdf": build([SP_LF], LF, trailing_space=True),
        # Old-Mac: every EOL is a bare CR (entries use the legal SP CR form).
        "bare-cr-oldmac.pdf": build([SP_CR], CR),
        # Mixed: entries rotate all three legal EOLs; structural lines
        # rotate CR/LF/CRLF. Proves no single-EOL assumption anywhere.
        "mixed-eol.pdf": build(
            [SP_CR, SP_LF, CR_LF],
            [b"\r", b"\n", b"\r\n"],
        ),
    }

    for name, data in fixtures.items():
        path = os.path.join(OUT, name)
        with open(path, "wb") as fh:
            fh.write(data)
        print(f"wrote {path} ({len(data)} bytes)")


if __name__ == "__main__":
    main()
