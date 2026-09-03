#!/usr/bin/env python3
"""Generate the synthetic embedded-file (attachment) fixtures.

WHY THIS SCRIPT EXISTS
----------------------
``crates/pdfcer-core/src/attachments.rs`` reads a document's attachments,
and a PDF has **two structurally unrelated** ways of carrying one:

1. **document-level** — a §7.9.6 name tree hanging off the catalog at
   ``/Names /EmbeddedFiles``, whose values are §7.11.3 file-specification
   dictionaries (ISO 32000-1 §7.11.4);
2. **page-level** — a §12.5.6.15 ``/FileAttachment`` annotation, whose
   ``/FS`` entry is a file specification pinned to a rectangle on one
   page.

Conflating those two is the central hazard of the feature: they differ in
lifetime (deleting a page destroys the second and not the first), in save
behaviour, and in who is allowed to author them. So every fixture here
isolates ONE claim about that reader, and a failing test names the claim
it broke rather than reporting that "attachments are wrong".

`docs/LEGAL.md` §5 permits only synthetic or rights-cleared PDFs under
``fixtures/``. Every byte written here is constructed in this file, from
nothing, with no PDF library behind it — so a fixture cannot inherit a
bug (or a helpful normalisation) from the very code it exists to test.
This follows the established ``tools/gen-*-fixtures.py`` pattern exactly:
a deliberately minimal writer emitting a **classic** cross-reference
table (§7.5.4), a ``%PDF-1.7`` header and the §7.5.2 binary comment.

The one non-hand-rolled thing is ``zlib.compress`` for the single
FlateDecode fixture (§7.4.4 delegates FlateDecode to RFC 1950), because
the point of that fixture is precisely that the *encoded* length and the
*decoded* length differ, and hand-authoring a deflate stream would add
nothing but the chance of a typo.

WHAT IT WRITES
--------------
``fixtures/synthetic/attachments/`` — nine files.

``doc-level-simple.pdf``
    One document-level embedded file in a **flat** name tree (the root
    node carries ``/Names`` directly and has no ``/Kids``, which §7.9.6
    permits). The file specification exercises every optional key a
    reader is expected to surface: ``/F`` (an ASCII byte string),
    ``/UF`` (the same name as a UTF-16BE text string, which §7.11.3
    says a PDF 1.7 reader should prefer), ``/Desc``, and a ``/Params``
    dictionary with ``/Size``, ``/CreationDate``, ``/ModDate`` and
    ``/CheckSum``. The embedded stream declares ``/Subtype /text#2Fplain``
    — a name whose ``#2F`` escape is the ``/`` of the MIME type, since
    ``/`` cannot appear literally in a name (§7.3.5). A reader that does
    not decode ``#`` escapes reports the MIME type wrong.

``doc-level-unicode-name.pdf``
    ``/UF`` is a genuinely non-ASCII UTF-16BE name (``ré​sumé-Σ.txt``)
    while ``/F`` carries a lossy ASCII fallback (``resume.txt``) and the
    NAME-TREE KEY is a third, different string. All three disagree on
    purpose: this pins the precedence order and makes a reader that
    silently falls back to the key, or that treats ``/UF`` bytes as
    Latin-1, fail visibly.

``doc-level-kids-tree.pdf``
    A three-leaf name tree with a real interior: root → ``/Kids`` → two
    intermediate nodes → three leaves, every non-root node carrying
    ``/Limits``. A reader that only looks at the root's ``/Names`` finds
    nothing at all here, which is the defect this catches.

``page-level-annot.pdf``
    A single page carrying one §12.5.6.15 ``/FileAttachment``
    annotation (``/Name /Paperclip``, ``/Contents`` describing it). Its
    ``/FS`` is an ordinary filespec with an ``/EF /F`` stream. The
    catalog has **no** ``/Names`` at all, so a reader that only walks
    the document-level tree reports this document as having no
    attachments — the exact conflation failure the module warns about.

``both-kinds.pdf``
    Two pages. One document-level entry, plus one ``/FileAttachment``
    annotation on **page 2** (zero-based page index 1). A reader must
    return both, label them by kind, and get the page index right; a
    reader that hard-codes page 0, or that reports two document-level
    entries, fails here.

``size-lies.pdf``
    ``/Params /Size 999999`` on a stream whose actual payload is 11
    bytes, uncompressed. §7.11.4 makes ``/Size`` an *optional, declared*
    number; nothing verifies it. This is the fixture for "report the
    declared size AS declared, and disclose the disagreement".

``flate-size-truth.pdf``
    A FlateDecode-compressed embedded file whose ``/Params /Size`` is the
    **decoded** size and whose ``/Length`` is the (different) encoded
    size. A reader that validates ``/Size`` against ``/Length`` instead
    of against the decoded byte count reports a false mismatch here,
    which is why this fixture is the twin of ``size-lies.pdf``: one must
    be reported as disagreeing and the other must not.

``annot-contents-beats-desc.pdf``
    ONE filespec, carrying a ``/Desc``, reached BOTH ways: as a
    ``/Names /EmbeddedFiles`` value and as an annotation's ``/FS``. The
    annotation's ``/Contents`` differs from the ``/Desc``. §12.5.6.15
    makes ``/Contents`` win **for the annotation route only**, so the
    correct reading is two rows with two different descriptions off one
    shared dictionary.

``ef-platform-slots.pdf``
    The shape ISO 32000-1's own §7.11.4 EXAMPLE uses: a filespec with no
    ``/F`` and no ``/UF``, only the obsolescent ``/DOS``/``/Mac``/
    ``/Unix`` slots, and an ``/EF`` containing only ``/Unix``. Table 44
    makes ``/F`` required *only* when all three platform entries are
    absent, so this is conforming — and a reader that consults only
    ``/F``/``/UF`` finds a nameless attachment with no bytes.

``hostile-names.pdf``
    Four document-level entries whose names are attacker-shaped:
    a Windows path traversal (``..\\..\\..\\Windows\\System32\\evil.exe``),
    a POSIX absolute path (``/etc/cron.d/pwn``), an embedded NUL byte
    (``ok.txt\\x00.exe`` — the classic extension-spoof), and a reserved
    Windows device name (``CON.txt``). None of these are illegal PDF;
    §7.11.2's file-specification string format does not forbid them, and
    a name-tree key is an arbitrary byte string. The fixture exists so
    the sanitiser has something real to be tested against, and so a
    future "extract all" feature cannot be written without meeting them.

``degenerate.pdf``
    Everything a malformed or merely unusual document can do to this
    reader, in one file, so the "never panic, degrade" contract has a
    single home: a name tree whose ``/Kids`` points back at the root (a
    cycle), a filespec with **no** ``/EF`` at all (legal — §7.11.3
    describes external file references), a filespec whose ``/EF /F``
    is a dangling reference (§7.3.10: null, "shall not be considered an
    error"), a ``/Names`` array with an odd number of elements, and an
    entry whose value is an integer rather than a dictionary.

Regenerate with ``python tools/gen-attachment-fixtures.py``. CC0.
"""

from __future__ import annotations

import zlib
from pathlib import Path

OUT_DIR = Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "attachments"


# ---------------------------------------------------------------------------
# Minimal byte-level PDF writer.
#
# Deliberately dumb: objects are appended in numeric order, offsets are
# recorded as they go, and the xref is one classic §7.5.4 subsection
# starting at object 0. There is no object-stream support, no compression
# except where a fixture explicitly asks for it, and no cleverness that
# could paper over a defect in the code under test.
# ---------------------------------------------------------------------------


class Pdf:
    """Accumulates numbered objects and serialises a §7.5.4 classic file."""

    def __init__(self) -> None:
        self._objects: dict[int, bytes] = {}
        self._next = 1

    def reserve(self) -> int:
        """Claim the next object number without writing its body yet.

        Needed because filespecs and their embedded streams reference each
        other's numbers, and because the catalog must be object 1 for
        readability while being written last.
        """
        num = self._next
        self._next += 1
        return num

    def put(self, num: int, body: bytes) -> int:
        """Store `body` as the content of object `num` (between `obj`/`endobj`)."""
        self._objects[num] = body
        return num

    def add(self, body: bytes) -> int:
        """Reserve a number and store `body` under it."""
        return self.put(self.reserve(), body)

    def stream(self, num: int, dict_body: bytes, data: bytes) -> int:
        """Store a stream object.

        `dict_body` is the stream dictionary WITHOUT the surrounding
        ``<<``/``>>`` and WITHOUT ``/Length`` — this adds both, because
        §7.3.8.2 makes ``/Length`` required and getting it wrong is a
        different bug than the ones these fixtures are for.
        """
        head = b"<< " + dict_body + b" /Length " + str(len(data)).encode("ascii") + b" >>\n"
        return self.put(num, head + b"stream\n" + data + b"\nendstream")

    def serialise(self, root: int) -> bytes:
        out = bytearray()
        out += b"%PDF-1.7\n"
        # §7.5.2: a comment with four bytes >127 so transfer tools treat
        # the file as binary rather than text.
        out += b"%\xe2\xe3\xcf\xd3\n"

        offsets: dict[int, int] = {}
        for num in sorted(self._objects):
            offsets[num] = len(out)
            out += f"{num} 0 obj\n".encode("ascii")
            out += self._objects[num]
            out += b"\nendobj\n"

        highest = max(self._objects) if self._objects else 0
        xref_at = len(out)
        out += b"xref\n"
        out += f"0 {highest + 1}\n".encode("ascii")
        out += b"0000000000 65535 f \n"
        for num in range(1, highest + 1):
            if num in offsets:
                out += f"{offsets[num]:010d} 00000 n \n".encode("ascii")
            else:
                # §7.5.4 requires an entry for every number up to the
                # maximum even when the object does not occur; a free
                # entry is the conforming filler.
                out += b"0000000000 65535 f \n"
        out += (
            f"trailer\n<< /Size {highest + 1} /Root {root} 0 R >>\n"
            f"startxref\n{xref_at}\n%%EOF\n"
        ).encode("ascii")
        return bytes(out)


def pdf_string(raw: bytes) -> bytes:
    """Serialise `raw` as a §7.3.4.2 literal string with full escaping.

    Every byte outside the printable-ASCII range, and every byte that
    would terminate or nest the literal, is written as a ``\\ddd`` octal
    escape. That is what lets a fixture carry a NUL or a backslash
    verbatim without the file becoming unparseable — which matters,
    because ``hostile-names.pdf`` exists to carry exactly those bytes.
    """
    out = bytearray(b"(")
    for b in raw:
        if b in (0x28, 0x29, 0x5C):  # ( ) \
            out += b"\\" + bytes([b])
        elif 0x20 <= b <= 0x7E:
            out.append(b)
        else:
            out += f"\\{b:03o}".encode("ascii")
    out += b")"
    return bytes(out)


def utf16be(text: str) -> bytes:
    """A §7.9.2.2 text string in UTF-16BE, BOM included, as a literal string."""
    return pdf_string(b"\xfe\xff" + text.encode("utf-16-be"))


def date(value: str) -> bytes:
    """A §7.9.4 date string, e.g. ``D:20260810123000Z``."""
    return pdf_string(value.encode("ascii"))


# ---------------------------------------------------------------------------
# Shared builders.
# ---------------------------------------------------------------------------


def add_embedded_file(
    pdf: Pdf,
    data: bytes,
    *,
    subtype: bytes | None = b"/text#2Fplain",
    size: int | None = None,
    created: str | None = "D:20260101000000Z",
    modified: str | None = "D:20260810123000Z",
    checksum: bytes | None = None,
    compress: bool = False,
) -> int:
    """Write a §7.11.4 embedded file stream and return its object number.

    `size` defaults to the true decoded length. Passing something else is
    how ``size-lies.pdf`` gets built — the parameter exists so the lie is
    written by the fixture that wants it and nowhere else.
    """
    num = pdf.reserve()
    params = bytearray()
    if size is None:
        size = len(data)
    params += b" /Size " + str(size).encode("ascii")
    if created is not None:
        params += b" /CreationDate " + date(created)
    if modified is not None:
        params += b" /ModDate " + date(modified)
    if checksum is not None:
        params += b" /CheckSum " + pdf_string(checksum)

    body = bytearray(b"/Type /EmbeddedFile")
    if subtype is not None:
        body += b" /Subtype " + subtype
    body += b" /Params << " + bytes(params).strip() + b" >>"

    payload = data
    if compress:
        payload = zlib.compress(data, 9)
        body += b" /Filter /FlateDecode"

    pdf.stream(num, bytes(body), payload)
    return num


def filespec(
    pdf: Pdf,
    *,
    f: bytes | None = None,
    uf: bytes | None = None,
    desc: bytes | None = None,
    ef: int | None = None,
    ef_dangling: int | None = None,
) -> int:
    """Write a §7.11.3 file-specification dictionary and return its number.

    `f`/`uf`/`desc` are already-serialised PDF strings (so a caller can
    choose literal vs UTF-16BE per key). `ef` is the object number of an
    embedded file stream; `ef_dangling` writes an ``/EF /F`` reference to
    a number that is deliberately never defined.
    """
    body = bytearray(b"/Type /Filespec")
    if f is not None:
        body += b" /F " + f
    if uf is not None:
        body += b" /UF " + uf
    if desc is not None:
        body += b" /Desc " + desc
    if ef is not None:
        body += b" /EF << /F " + str(ef).encode("ascii") + b" 0 R >>"
    elif ef_dangling is not None:
        body += b" /EF << /F " + str(ef_dangling).encode("ascii") + b" 0 R >>"
    return pdf.add(b"<< " + bytes(body) + b" >>")


def simple_page_tree(pdf: Pdf, count: int = 1) -> tuple[int, list[int]]:
    """A `/Pages` node with `count` blank 200x200 leaves.

    Returns ``(pages_num, [page_nums])``. Page objects are written with a
    placeholder body first so a caller can rewrite one to add `/Annots`.
    """
    pages_num = pdf.reserve()
    page_nums = [pdf.reserve() for _ in range(count)]
    for num in page_nums:
        pdf.put(
            num,
            b"<< /Type /Page /Parent "
            + str(pages_num).encode("ascii")
            + b" 0 R /MediaBox [0 0 200 200] /Resources << >> >>",
        )
    kids = b" ".join(str(n).encode("ascii") + b" 0 R" for n in page_nums)
    pdf.put(
        pages_num,
        b"<< /Type /Pages /Count "
        + str(count).encode("ascii")
        + b" /Kids ["
        + kids
        + b"] >>",
    )
    return pages_num, page_nums


def catalog(pdf: Pdf, pages_num: int, *, names: bytes | None = None) -> int:
    """The document catalog (§7.7.2), optionally carrying `/Names`."""
    body = bytearray(b"/Type /Catalog /Pages " + str(pages_num).encode("ascii") + b" 0 R")
    if names is not None:
        body += b" /Names " + names
    return pdf.add(b"<< " + bytes(body) + b" >>")


def attach_annotation(
    pdf: Pdf,
    page_num: int,
    pages_num: int,
    fs_num: int,
    *,
    icon: bytes = b"/Paperclip",
    contents: bytes = b"(A file pinned to this page)",
) -> None:
    """Rewrite `page_num` to carry one §12.5.6.15 /FileAttachment annotation."""
    annot = pdf.add(
        b"<< /Type /Annot /Subtype /FileAttachment"
        b" /Rect [20 150 40 170] /F 4"
        b" /Name " + icon + b" /Contents " + contents + b" /FS "
        + str(fs_num).encode("ascii")
        + b" 0 R >>"
    )
    pdf.put(
        page_num,
        b"<< /Type /Page /Parent "
        + str(pages_num).encode("ascii")
        + b" 0 R /MediaBox [0 0 200 200] /Resources << >>"
        b" /Annots [" + str(annot).encode("ascii") + b" 0 R] >>",
    )


# ---------------------------------------------------------------------------
# The fixtures.
# ---------------------------------------------------------------------------


def doc_level_simple() -> bytes:
    pdf = Pdf()
    pages_num, _ = simple_page_tree(pdf)
    data = b"hello attachment\n"
    ef = add_embedded_file(pdf, data, checksum=b"\x01\x02\x03\x04")
    fs = filespec(
        pdf,
        f=pdf_string(b"notes.txt"),
        uf=utf16be("notes.txt"),
        desc=utf16be("A plain-text note"),
        ef=ef,
    )
    names = pdf.add(
        b"<< /EmbeddedFiles << /Names ["
        + utf16be("notes.txt")
        + b" "
        + str(fs).encode("ascii")
        + b" 0 R] >> >>"
    )
    root = catalog(pdf, pages_num, names=str(names).encode("ascii") + b" 0 R")
    return pdf.serialise(root)


def doc_level_unicode_name() -> bytes:
    pdf = Pdf()
    pages_num, _ = simple_page_tree(pdf)
    ef = add_embedded_file(pdf, b"CV contents\n")
    fs = filespec(
        pdf,
        # Three different spellings on purpose — see the module docstring.
        f=pdf_string(b"resume.txt"),
        uf=utf16be("résumé-Σ.txt"),
        desc=utf16be("Curriculum vitae"),
        ef=ef,
    )
    names = pdf.add(
        b"<< /EmbeddedFiles << /Names ["
        + pdf_string(b"tree-key-differs.txt")
        + b" "
        + str(fs).encode("ascii")
        + b" 0 R] >> >>"
    )
    root = catalog(pdf, pages_num, names=str(names).encode("ascii") + b" 0 R")
    return pdf.serialise(root)


def doc_level_kids_tree() -> bytes:
    """Root → two intermediate nodes → three leaves, all with /Limits."""
    pdf = Pdf()
    pages_num, _ = simple_page_tree(pdf)

    specs = []
    for name in (b"alpha.txt", b"mike.txt", b"zulu.txt"):
        ef = add_embedded_file(pdf, b"payload for " + name + b"\n")
        specs.append((name, filespec(pdf, f=pdf_string(name), uf=utf16be(name.decode()), ef=ef)))

    def leaf(entries: list[tuple[bytes, int]]) -> int:
        pairs = b" ".join(
            pdf_string(k) + b" " + str(v).encode("ascii") + b" 0 R" for k, v in entries
        )
        lo, hi = entries[0][0], entries[-1][0]
        return pdf.add(
            b"<< /Limits [" + pdf_string(lo) + b" " + pdf_string(hi) + b"]"
            b" /Names [" + pairs + b"] >>"
        )

    leaf_a = leaf(specs[0:1])
    leaf_b = leaf(specs[1:3])
    mid = pdf.add(
        b"<< /Limits ["
        + pdf_string(specs[1][0])
        + b" "
        + pdf_string(specs[2][0])
        + b"] /Kids ["
        + str(leaf_b).encode("ascii")
        + b" 0 R] >>"
    )
    # §7.9.6: the ROOT node has no /Limits. Only /Kids here — a reader
    # that reads /Names off the root finds nothing.
    tree_root = pdf.add(
        b"<< /Kids ["
        + str(leaf_a).encode("ascii")
        + b" 0 R "
        + str(mid).encode("ascii")
        + b" 0 R] >>"
    )
    names = pdf.add(
        b"<< /EmbeddedFiles " + str(tree_root).encode("ascii") + b" 0 R >>"
    )
    root = catalog(pdf, pages_num, names=str(names).encode("ascii") + b" 0 R")
    return pdf.serialise(root)


def page_level_annot() -> bytes:
    pdf = Pdf()
    pages_num, page_nums = simple_page_tree(pdf)
    ef = add_embedded_file(pdf, b"pinned to page one\n")
    fs = filespec(pdf, f=pdf_string(b"pinned.txt"), uf=utf16be("pinned.txt"), ef=ef)
    attach_annotation(pdf, page_nums[0], pages_num, fs)
    # NO /Names in the catalog: this document's only attachment is the
    # annotation, and a document-level-only reader must come up empty.
    root = catalog(pdf, pages_num)
    return pdf.serialise(root)


def both_kinds() -> bytes:
    pdf = Pdf()
    pages_num, page_nums = simple_page_tree(pdf, count=2)

    doc_ef = add_embedded_file(pdf, b"document-level payload\n")
    doc_fs = filespec(
        pdf, f=pdf_string(b"whole-document.txt"), uf=utf16be("whole-document.txt"), ef=doc_ef
    )

    page_ef = add_embedded_file(pdf, b"page-level payload\n")
    page_fs = filespec(
        pdf, f=pdf_string(b"on-page-two.txt"), uf=utf16be("on-page-two.txt"), ef=page_ef
    )
    # Page index 1 (the SECOND page) deliberately: a reader that hard-codes
    # page 0 passes on page-level-annot.pdf and fails here.
    attach_annotation(pdf, page_nums[1], pages_num, page_fs)

    names = pdf.add(
        b"<< /EmbeddedFiles << /Names ["
        + utf16be("whole-document.txt")
        + b" "
        + str(doc_fs).encode("ascii")
        + b" 0 R] >> >>"
    )
    root = catalog(pdf, pages_num, names=str(names).encode("ascii") + b" 0 R")
    return pdf.serialise(root)


def size_lies() -> bytes:
    pdf = Pdf()
    pages_num, _ = simple_page_tree(pdf)
    data = b"eleven byt"  # 10 bytes; /Size will claim 999999.
    ef = add_embedded_file(pdf, data, size=999_999)
    fs = filespec(pdf, f=pdf_string(b"liar.bin"), uf=utf16be("liar.bin"), ef=ef)
    names = pdf.add(
        b"<< /EmbeddedFiles << /Names ["
        + utf16be("liar.bin")
        + b" "
        + str(fs).encode("ascii")
        + b" 0 R] >> >>"
    )
    root = catalog(pdf, pages_num, names=str(names).encode("ascii") + b" 0 R")
    return pdf.serialise(root)


def flate_size_truth() -> bytes:
    pdf = Pdf()
    pages_num, _ = simple_page_tree(pdf)
    # Highly compressible, so /Length and the decoded size are far apart
    # and a reader comparing /Size to /Length cannot pass by luck.
    data = b"A" * 4096
    ef = add_embedded_file(pdf, data, compress=True)
    fs = filespec(pdf, f=pdf_string(b"squeezed.txt"), uf=utf16be("squeezed.txt"), ef=ef)
    names = pdf.add(
        b"<< /EmbeddedFiles << /Names ["
        + utf16be("squeezed.txt")
        + b" "
        + str(fs).encode("ascii")
        + b" 0 R] >> >>"
    )
    root = catalog(pdf, pages_num, names=str(names).encode("ascii") + b" 0 R")
    return pdf.serialise(root)


HOSTILE = [
    # (name-tree key, /F bytes) — the key and /F agree here so a reader
    # cannot dodge the hazard by preferring one over the other.
    b"..\\..\\..\\Windows\\System32\\evil.exe",
    b"/etc/cron.d/pwn",
    b"ok.txt\x00.exe",
    b"CON.txt",
]


def hostile_names() -> bytes:
    pdf = Pdf()
    pages_num, _ = simple_page_tree(pdf)
    entries = []
    for raw in HOSTILE:
        ef = add_embedded_file(pdf, b"inert payload\n", subtype=b"/application#2Foctet-stream")
        fs = filespec(pdf, f=pdf_string(raw), ef=ef)
        entries.append((raw, fs))
    pairs = b" ".join(
        pdf_string(k) + b" " + str(v).encode("ascii") + b" 0 R" for k, v in entries
    )
    names = pdf.add(b"<< /EmbeddedFiles << /Names [" + pairs + b"] >> >>")
    root = catalog(pdf, pages_num, names=str(names).encode("ascii") + b" 0 R")
    return pdf.serialise(root)


def annot_contents_beats_desc() -> bytes:
    """The §12.5.6.15 ``shall``, isolated.

    ONE filespec, carrying a ``/Desc``, is reached BOTH ways: it is the
    value of a ``/Names /EmbeddedFiles`` entry AND the ``/FS`` of a
    ``/FileAttachment`` annotation on page 1. The annotation's
    ``/Contents`` says something different.

    §12.5.6.15: "Conforming readers **shall use this entry rather than
    the optional Desc entry (PDF 1.6) in the file specification
    dictionary**." So the correct reading is two rows with two different
    descriptions off one dictionary — and a reader that reads ``/Desc``
    for both, or that de-duplicates the rows because they share a
    filespec, gets it wrong.
    """
    pdf = Pdf()
    pages_num, page_nums = simple_page_tree(pdf)
    ef = add_embedded_file(pdf, b"shared payload\n")
    fs = filespec(
        pdf,
        f=pdf_string(b"shared.txt"),
        uf=utf16be("shared.txt"),
        desc=utf16be("DESC from the file specification"),
        ef=ef,
    )
    attach_annotation(
        pdf,
        page_nums[0],
        pages_num,
        fs,
        contents=utf16be("CONTENTS from the annotation"),
    )
    names = pdf.add(
        b"<< /EmbeddedFiles << /Names ["
        + utf16be("shared.txt")
        + b" "
        + str(fs).encode("ascii")
        + b" 0 R] >> >>"
    )
    root = catalog(pdf, pages_num, names=str(names).encode("ascii") + b" 0 R")
    return pdf.serialise(root)


def ef_platform_slots() -> bytes:
    """The shape ISO 32000-1's own §7.11.4 EXAMPLE uses.

    A filespec with **no** ``/F`` or ``/UF`` — only the obsolescent
    ``/DOS``, ``/Mac`` and ``/Unix`` slots — and an ``/EF`` that likewise
    has no ``/F``. Table 44 makes ``/F`` "required if the DOS, Mac, and
    Unix entries are all absent", so supplying only platform slots is
    **conforming**, and a reader that looks at ``/F``/``/UF`` alone finds
    a nameless attachment with no bytes in a perfectly legal document.

    The three names differ so the slot precedence is observable.
    """
    pdf = Pdf()
    pages_num, _ = simple_page_tree(pdf)
    ef = add_embedded_file(pdf, b"platform payload\n")
    num = pdf.add(
        b"<< /Type /Filespec"
        b" /DOS " + pdf_string(b"DOSNAME.TXT") + b" /Mac "
        + pdf_string(b"MacName.txt")
        + b" /Unix "
        + pdf_string(b"unix-name.txt")
        + b" /EF << /Unix "
        + str(ef).encode("ascii")
        + b" 0 R >> >>"
    )
    names = pdf.add(
        b"<< /EmbeddedFiles << /Names ["
        + pdf_string(b"platform.txt")
        + b" "
        + str(num).encode("ascii")
        + b" 0 R] >> >>"
    )
    root = catalog(pdf, pages_num, names=str(names).encode("ascii") + b" 0 R")
    return pdf.serialise(root)


def degenerate() -> bytes:
    """Five malformations in one document; see the module docstring."""
    pdf = Pdf()
    pages_num, _ = simple_page_tree(pdf)

    # (a) a perfectly good entry, so "degraded" can be distinguished from
    #     "gave up on the whole document".
    good_ef = add_embedded_file(pdf, b"survivor\n")
    good_fs = filespec(pdf, f=pdf_string(b"good.txt"), uf=utf16be("good.txt"), ef=good_ef)

    # (b) a filespec with NO /EF: §7.11.3 describes external file
    #     references, which are legal and simply have no bytes.
    external_fs = filespec(pdf, f=pdf_string(b"elsewhere.txt"))

    # (c) a filespec whose /EF /F points at an object that does not exist.
    #     §7.3.10 makes that null, not an error.
    dangling_fs = filespec(pdf, f=pdf_string(b"vanished.txt"), ef_dangling=9999)

    tree_root_num = pdf.reserve()

    # (d) a leaf whose /Names array has an ODD length (a key with no
    #     value) and an entry whose value is an integer, not a filespec.
    odd_leaf = pdf.add(
        b"<< /Names ["
        + pdf_string(b"good.txt")
        + b" "
        + str(good_fs).encode("ascii")
        + b" 0 R "
        + pdf_string(b"elsewhere.txt")
        + b" "
        + str(external_fs).encode("ascii")
        + b" 0 R "
        + pdf_string(b"vanished.txt")
        + b" "
        + str(dangling_fs).encode("ascii")
        + b" 0 R "
        + pdf_string(b"not-a-filespec")
        + b" 42 "
        + pdf_string(b"dangling-key-no-value")
        + b"] >>"
    )

    # (e) a /Kids that points back at the tree root: a cycle. A reader
    #     without a visited-set recurses until its depth guard fires on
    #     every branch, which is quadratic rather than merely bounded.
    cyclic_kid = pdf.add(
        b"<< /Kids [" + str(tree_root_num).encode("ascii") + b" 0 R] >>"
    )
    pdf.put(
        tree_root_num,
        b"<< /Kids ["
        + str(odd_leaf).encode("ascii")
        + b" 0 R "
        + str(cyclic_kid).encode("ascii")
        + b" 0 R] >>",
    )

    names = pdf.add(b"<< /EmbeddedFiles " + str(tree_root_num).encode("ascii") + b" 0 R >>")
    root = catalog(pdf, pages_num, names=str(names).encode("ascii") + b" 0 R")
    return pdf.serialise(root)


FIXTURES = {
    "doc-level-simple.pdf": doc_level_simple,
    "doc-level-unicode-name.pdf": doc_level_unicode_name,
    "doc-level-kids-tree.pdf": doc_level_kids_tree,
    "page-level-annot.pdf": page_level_annot,
    "both-kinds.pdf": both_kinds,
    "size-lies.pdf": size_lies,
    "flate-size-truth.pdf": flate_size_truth,
    "annot-contents-beats-desc.pdf": annot_contents_beats_desc,
    "ef-platform-slots.pdf": ef_platform_slots,
    "hostile-names.pdf": hostile_names,
    "degenerate.pdf": degenerate,
}


def main() -> None:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    for name, build in FIXTURES.items():
        data = build()
        (OUT_DIR / name).write_bytes(data)
        print(f"wrote {OUT_DIR / name} ({len(data)} bytes)")


if __name__ == "__main__":
    main()
