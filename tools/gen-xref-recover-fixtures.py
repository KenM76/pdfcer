#!/usr/bin/env python3
"""Generate synthetic cross-reference RECOVERY fixtures (decision 013 Pass B).

Pass B of pdfcer decision 013 adds a rebuild-by-scan fallback: when a file's
stored cross-reference machinery cannot be parsed, pdfcer scans the whole
buffer for `N G obj` headers and reconstructs the xref + trailer. These
fixtures are the positive/negative controls for that path — each is a
deliberately-damaged (or headerless) document that STRICT loading rejects,
and that recovery must either open (rebuild-by-scan) or refuse cleanly.

Every fixture is self-authored and synthetic — LEGAL §5 compliant (no
third-party content). Regenerate with
`python tools/gen-xref-recover-fixtures.py`. CC0.

## The matrix

| file                          | damage                                   | expected recovery                         |
|-------------------------------|------------------------------------------|-------------------------------------------|
| offset-shifted-startxref.pdf  | `startxref` lands inside an object       | OPENS (NotAnXrefSection) — the qpdf shape |
| no-startxref.pdf              | no `startxref`/`%%EOF` at all            | OPENS (StartxrefNotFound)                 |
| startxref-out-of-range.pdf    | `startxref` value past EOF               | OPENS (BadStartxrefOffset)                |
| xref-stream-corrupt.pdf       | pure xref-stream, stream data undecodable| OPENS (XrefStreamDecode) — file-lvl+ObjStm|
| duplicate-superseded.pdf      | object 3 defined twice + no `startxref`  | OPENS, last-wins picks the 2nd def        |
| offset-start.pdf              | >1 KiB leading junk before `%PDF-`       | OPENS (MissingHeader, offset_start)       |
| header-preamble.pdf           | 12-byte preamble, offsets absolute      | OPENS STRICT; a full rewrite must drop it |
| unrecoverable-no-catalog.pdf  | objects but no `/Type /Catalog`, no sxr  | REFUSES clean (NoCatalog)                 |

The `offset-shifted-startxref` fixture reproduces the canonical real-world
case: `qpdf/add-contents.pdf` stores `startxref 685` but byte 685 lands
inside `...endobj\r\n8 0 obj` (a 39-byte LF→CRLF forward shift). Here the
shift is synthesized directly by pointing `startxref` at a body object's
dictionary, which strict classification rejects as `NotAnXrefSection`.
"""
import os

OUT = "fixtures/synthetic/xref-recover"

HEADER = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n"


def one_page_objects():
    """Objects 1..4 of a minimal resolvable one-page document.

    1 catalog, 2 page tree, 3 page, 4 content stream. Object 1 is the
    catalog so the trailer's `/Root 1 0 R` (and recovery's catalog search)
    resolve.
    """
    content = b"BT /F1 12 Tf 72 720 Td (xref recovery fixture) Tj ET"
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


def emit_bodies(objs):
    """Append `N G obj … endobj` for each object; return (buf, offsets)."""
    buf = bytearray(HEADER)
    off = {}
    for n in sorted(objs):
        off[n] = len(buf)
        buf += b"%d 0 obj\n" % n + objs[n] + b"\nendobj\n"
    return buf, off


def classic_xref(off, size):
    """A well-formed single-subsection classic `xref` block (no trailer)."""
    x = bytearray(b"xref\n0 %d\n" % size)
    x += b"0000000000 65535 f \n"  # free-list head
    for num in range(1, size):
        x += b"%010d 00000 n \n" % off[num]
    return x


def classic_trailer(size, root=1):
    return b"trailer\n<< /Size %d /Root %d 0 R >>\n" % (size, root)


def build_valid_classic():
    """A complete, VALID classic one-page PDF. Returns (buf, xref_at, off)."""
    objs = one_page_objects()
    buf, off = emit_bodies(objs)
    size = max(objs) + 1
    xref_at = len(buf)
    buf += classic_xref(off, size)
    buf += classic_trailer(size)
    startxref_at = len(buf)
    buf += b"startxref\n%d\n%%%%EOF\n" % xref_at
    return bytes(buf), xref_at, off, startxref_at


# --------------------------------------------------------------------------
# Individual fixtures
# --------------------------------------------------------------------------


def fx_offset_shifted_startxref():
    """`startxref` points at object 1's dictionary `<<`, not the real xref.

    Strict classification sees `DictOpen` there → NotAnXrefSection. Every
    other byte is a valid classic file, so this is a 'valid file pdfcer
    wrongly rejected' — recovery must open it. (The qpdf/add-contents.pdf
    offset-shift shape.)
    """
    objs = one_page_objects()
    buf, off = emit_bodies(objs)
    size = max(objs) + 1
    xref_at = len(buf)
    buf += classic_xref(off, size)
    buf += classic_trailer(size)
    # Point startxref at the `<<` of object 1 (after `1 0 obj\n`, 8 bytes).
    bad = off[1] + len(b"1 0 obj\n")
    buf += b"startxref\n%d\n%%%%EOF\n" % bad
    return bytes(buf)


def fx_no_startxref():
    """A valid classic file with the `startxref`/`%%EOF` lines removed.

    Strict → StartxrefNotFound. Recovery finds the objects + the `trailer`
    keyword and rebuilds.
    """
    objs = one_page_objects()
    buf, off = emit_bodies(objs)
    size = max(objs) + 1
    buf += classic_xref(off, size)
    buf += classic_trailer(size)
    # No startxref / %%EOF at all.
    return bytes(buf)


def fx_startxref_out_of_range():
    """`startxref` value points past EOF. Strict → BadStartxrefOffset."""
    objs = one_page_objects()
    buf, off = emit_bodies(objs)
    size = max(objs) + 1
    xref_at = len(buf)
    buf += classic_xref(off, size)
    buf += classic_trailer(size)
    huge = len(buf) + 1_000_000
    buf += b"startxref\n%d\n%%%%EOF\n" % huge
    return bytes(buf)


def fx_xref_stream_corrupt():
    """A pure xref-stream file whose xref stream DATA is undecodable, plus an
    UNcompressed object stream carrying object 6.

    Strict: startxref → the `/Type /XRef` object → decode fails
    (XrefStreamDecode). Recovery: scans objects 1-5,7 at file level, reads
    object 6 from the ObjStm's pair table, and lifts the trailer (/Root)
    from the `/Type /XRef` dictionary (whose DATA was bad but whose
    dictionary parsed).
    """
    buf = bytearray(b"%PDF-1.5\n%\xe2\xe3\xcf\xd3\n")
    off = {}

    def obj(n, body):
        off[n] = len(buf)
        buf.extend(b"%d 0 obj\n" % n + body + b"\nendobj\n")

    content = b"BT /F1 12 Tf 72 720 Td (xref-stream recovery) Tj ET"
    obj(1, b"<< /Type /Catalog /Pages 2 0 R >>")
    obj(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
    obj(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
        b"/Contents 4 0 R /Resources << >> >>",
    )
    obj(4, b"<< /Length %d >>\nstream\n%s\nendstream" % (len(content), content))

    # Object stream (obj 5), UNcompressed, holding object 6.
    member6 = b"<< /Type /ExtGState /CA 1 >>"
    pairs = b"6 0 "  # object 6 at offset 0 within the object region
    first = len(pairs)
    objstm_data = pairs + member6
    obj(
        5,
        b"<< /Type /ObjStm /N 1 /First %d /Length %d >>\nstream\n%s\nendstream"
        % (first, len(objstm_data), objstm_data),
    )

    # The cross-reference stream (obj 7): a well-formed /Type /XRef
    # dictionary (carrying /Root) whose FlateDecode data is garbage, so
    # strict decode fails. Recovery reads only the dictionary.
    bad_data = b"not zlib data"
    off7 = len(buf)
    off[7] = off7
    buf.extend(
        b"7 0 obj\n<< /Type /XRef /Root 1 0 R /Size 8 /W [1 2 1] "
        b"/Filter /FlateDecode /Length %d >>\nstream\n%s\nendstream\nendobj\n"
        % (len(bad_data), bad_data)
    )
    buf.extend(b"startxref\n%d\n%%%%EOF\n" % off7)
    return bytes(buf)


def fx_duplicate_superseded():
    """Object 3 defined twice (an incremental-update shape) and no
    `startxref`. Recovery's last-valid-wins must pick the SECOND definition.
    """
    objs = one_page_objects()
    # Replace object 3's first body with a marker; a second definition
    # further down supersedes it.
    objs[3] = (
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
        b"/Resources << >> /Contents 4 0 R /Note (FIRST) >>"
    )
    buf, off = emit_bodies(objs)
    # A second definition of object 3 with /Note (SECOND).
    off[3] = len(buf)
    buf += (
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
        b"/Resources << >> /Contents 4 0 R /Note (SECOND) >>\nendobj\n"
    )
    size = max(objs) + 1
    buf += classic_xref(off, size)  # (points at the 2nd def; irrelevant — no sxr)
    buf += classic_trailer(size)
    # No startxref → strict fails → recovery.
    return bytes(buf)


def fx_offset_start():
    """A valid PDF preceded by >1 KiB of junk, so the `%PDF-` header is not
    within the probe window. Strict header probe fails; recovery is
    header-independent (absolute offsets) and opens it. offset_start=True.
    """
    valid, _xref_at, _off, _sx = build_valid_classic()
    junk = (b"%% leading junk that is not a PDF header\n" * 64)  # ~2.6 KiB
    assert len(junk) > 1024
    return junk + valid


def fx_header_preamble():
    """A VALID classic file preceded by a SHORT preamble, so the `%PDF-`
    header is still inside the 1 KiB probe window and the file loads on the
    STRICT path — unlike `offset-start.pdf`, whose >1 KiB junk defeats the
    probe and routes through recovery instead.

    Offsets are ABSOLUTE from byte 0, exactly as §7.5.4/§7.5.5 require, so
    the file is spec-correct as written. Nothing here is damaged.

    This is the veraPDF-gate control for the 2026-08-07 header-preamble fix.
    veraPDF cannot open this INPUT ("can not locate xref table") because it
    reads offsets as header-relative whenever a preamble exists. pdfcer's full
    rewrite must therefore emit `%PDF-` at byte 0 and drop the preamble, at
    which point both readings coincide and the OUTPUT parses everywhere.
    A regression restoring preamble preservation flips this file from
    `improved` to a regression in the sweep, so the gate keeps watching it.
    """
    junk = b"%% preamble\n"
    objs = one_page_objects()
    body, off = emit_bodies(objs)
    # Shift every recorded offset by the preamble so they stay ABSOLUTE.
    off = {n: o + len(junk) for n, o in off.items()}
    size = max(objs) + 1
    buf = bytearray(junk) + body
    xref_at = len(buf)
    buf += classic_xref(off, size)
    buf += classic_trailer(size)
    buf += b"startxref\n%d\n%%%%EOF\n" % xref_at
    return bytes(buf)


def fx_unrecoverable_no_catalog():
    """Objects present but NONE is a `/Type /Catalog`, no `trailer` keyword,
    no `startxref`. Recovery finds objects but no catalog → clean refusal.
    """
    buf = bytearray(HEADER)

    def obj(n, body):
        buf.extend(b"%d 0 obj\n" % n + body + b"\nendobj\n")

    obj(1, b"<< /Type /Pages /Kids [2 0 R] /Count 1 >>")
    obj(2, b"<< /Type /Page /Parent 1 0 R /MediaBox [0 0 612 792] >>")
    # No catalog, no trailer keyword, no startxref.
    return bytes(buf)


def fx_crlf_shifted_lengths():
    """A file that was VALID with LF line endings and then converted to CRLF.

    This is the single most common real-world shape behind the "page
    /Contents is neither a stream nor an array of streams" failure: a
    corpus census over 4,012 files found 341 unopenable that way, and in
    337 the content stream's `N G obj` header was physically present but
    had been dropped by recovery's confirmation step.

    Converting LF→CRLF damages the file twice over, and BOTH halves matter:

    1. Every stored byte offset shifts forward by one per preceding line,
       so `startxref` no longer lands on `xref` — strict classification
       fails with `NotAnXrefSection` and recovery fires. (Necessary, but
       on its own this fixture would be a duplicate of
       `offset-shifted-startxref.pdf`.)
    2. Every `/Length` was measured on the LF form, so a stream containing
       N internal line breaks is now N bytes LONGER than it claims. The
       declared extent lands mid-data, `endstream` is not where `/Length`
       points, and under strict parsing the whole object is dropped as a
       false positive — taking the page's only content stream with it.

    The content stream therefore carries deliberate internal newlines: with
    single-line content the `/Length` would survive conversion untouched
    and the fixture would not exercise point 2 at all.

    Expected: OPENS, all four objects recovered,
    `RecoveryReport::stream_lengths_recovered == 1`, and the page's text is
    extractable.
    """
    content = b"BT\n/F1 12 Tf\n72 720 Td\n(crlf shifted lengths) Tj\nET"
    objs = one_page_objects()
    # `/Length` is correct for the LF form and stale after conversion.
    objs[4] = b"<< /Length %d >>\nstream\n%s\nendstream" % (len(content), content)
    buf, off = emit_bodies(objs)
    size = max(objs) + 1
    xref_at = len(buf)
    buf += classic_xref(off, size)
    buf += classic_trailer(size)
    buf += b"startxref\n%d\n%%%%EOF\n" % xref_at
    # The damage itself: a whole-file LF→CRLF conversion, exactly as a
    # text-mode transfer or a naive line-ending normalizer would do it.
    # (The binary-comment bytes \xe2\xe3\xcf\xd3 contain no 0x0A, so this
    # cannot corrupt them.)
    return bytes(buf).replace(b"\n", b"\r\n")


def fx_dangling_contents():
    """A file with a PERFECTLY VALID cross-reference table whose page names
    a `/Contents` object that does not exist.

    Deliberately NOT damaged in its xref: this fixture must load on the
    clean, strict path with no recovery at all. That is what makes it the
    round-trip control for the degradation — a document that opens only
    because a dangling `/Contents` degraded must still satisfy
    `ARCHITECTURE.md` §5, and it can only prove that on the path where
    byte-identical incremental save is actually attempted (a *recovered*
    document refuses incremental save by name, so the recovery fixtures
    cannot test this).

    Object 4 is present in the table as a FREE entry — the well-formed way
    to say "this object number names nothing" (§7.5.4). Resolving `4 0 R`
    therefore yields the null object (§7.3.10), and Table 30's "if this
    entry is absent, the page shall be empty" makes the page empty rather
    than the document invalid.

    Expected: OPENS on the strict path, one page, `contents` empty,
    `contents_unresolved == 1`, and save byte-identical.
    """
    objs = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        3: (
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            b"/Resources << >> /Contents 4 0 R >>"
        ),
    }
    buf, off = emit_bodies(objs)
    size = 5  # 0..4 — object 4 is declared and FREE
    xref_at = len(buf)
    x = bytearray(b"xref\n0 %d\n" % size)
    x += b"0000000000 65535 f \n"  # free-list head
    for num in (1, 2, 3):
        x += b"%010d 00000 n \n" % off[num]
    x += b"0000000000 65535 f \n"  # object 4: free — the dangling target
    buf += x
    buf += classic_trailer(size)
    buf += b"startxref\n%d\n%%%%EOF\n" % xref_at
    return bytes(buf)


def fx_dangling_contents_array():
    """As `fx_dangling_contents`, but `/Contents` is an ARRAY whose middle
    element is the dangling one, flanked by two real streams.

    Proves the surviving elements still concatenate in order rather than
    the array being condemned wholesale — Table 30 divides a content
    stream only at lexical-token boundaries, so dropping one element
    leaves a shorter but syntactically intact stream. The two live streams
    are written so the result is only valid if BOTH are kept and in order
    (`q` … `Q` must balance).

    Expected: OPENS on the strict path, `contents == [4 0 R, 6 0 R]`,
    `contents_unresolved == 1`.
    """
    first = b"q 1 0 0 RG 10 10 m 100 100 l S"
    third = b"0 0 1 RG 10 100 m 100 10 l S Q"
    objs = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        3: (
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            b"/Resources << >> /Contents [4 0 R 5 0 R 6 0 R] >>"
        ),
        4: b"<< /Length %d >>\nstream\n%s\nendstream" % (len(first), first),
        6: b"<< /Length %d >>\nstream\n%s\nendstream" % (len(third), third),
    }
    buf, off = emit_bodies(objs)
    size = 7
    xref_at = len(buf)
    x = bytearray(b"xref\n0 %d\n" % size)
    x += b"0000000000 65535 f \n"
    for num in range(1, size):
        if num in off:
            x += b"%010d 00000 n \n" % off[num]
        else:
            # Object 5 is declared FREE: the dangling middle element.
            x += b"0000000000 65535 f \n"
    buf += x
    buf += classic_trailer(size)
    buf += b"startxref\n%d\n%%%%EOF\n" % xref_at
    return bytes(buf)


def fx_missing_endobj_on_page_tree():
    """The `/Pages` node's `endobj` is missing; everything else is valid.

    Models qpdf's `bad6.pdf`, reduced to the one thing that matters. The
    catalog says `/Pages 2 0 R`, and object 2's definition runs straight
    into `3 0 obj` with no terminator -- so a parser that requires
    `endobj` (ISO 32000-1 s7.3.10) drops object 2 as unparseable.

    WHY THIS FIXTURE EXISTS. Until 2026-08-07 pdfcer dropped it, and then
    WROTE a file whose catalog still said `/Pages 2 0 R` while object 2
    was absent -- a document strictly worse than the input, because
    veraPDF could recover the original and could not recover pdfcer's
    rewrite of it. Found by `tools/verapdf-parse-gate.py`, which was the
    first outside judge of pdfcer's own output.

    The xref is also removed, because that is what forces the recovery
    path where the leniency lives; with a valid xref the strict loader is
    used and must still refuse (see the paired test).
    """
    objs = one_page_objects()
    buf = bytearray(HEADER)
    for n in sorted(objs):
        buf += b"%d 0 obj\n" % n + objs[n]
        # Object 2 — and only object 2 — loses its terminator.
        buf += b"\n" if n == 2 else b"\nendobj\n"
    buf += b"trailer\n<< /Size %d /Root 1 0 R >>\n" % (max(objs) + 1)
    return bytes(buf)


FIXTURES = {
    "offset-shifted-startxref.pdf": fx_offset_shifted_startxref,
    "crlf-shifted-lengths.pdf": fx_crlf_shifted_lengths,
    "dangling-contents.pdf": fx_dangling_contents,
    "dangling-contents-array.pdf": fx_dangling_contents_array,
    "no-startxref.pdf": fx_no_startxref,
    "startxref-out-of-range.pdf": fx_startxref_out_of_range,
    "xref-stream-corrupt.pdf": fx_xref_stream_corrupt,
    "duplicate-superseded.pdf": fx_duplicate_superseded,
    "offset-start.pdf": fx_offset_start,
    "header-preamble.pdf": fx_header_preamble,
    "unrecoverable-no-catalog.pdf": fx_unrecoverable_no_catalog,
    "missing-endobj-page-tree.pdf": fx_missing_endobj_on_page_tree,
}


def main():
    os.makedirs(OUT, exist_ok=True)
    for name, fn in FIXTURES.items():
        data = fn()
        path = os.path.join(OUT, name)
        with open(path, "wb") as fh:
            fh.write(data)
        print(f"wrote {path} ({len(data)} bytes)")


if __name__ == "__main__":
    main()
