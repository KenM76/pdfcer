"""Generate the font-EMBEDDING fixtures for `pdfcer_core::font_embed_missing`
(Pass 67.0 phase E).

Phase A's `tools/gen-fontinfo-fixtures.py` covers the CLASSIFIER; phase B's
`tools/gen-unembed-fixtures.py` covers the structural hazards of DELETING a
font program. This generator covers the ones that only exist when pdfcer
ADDS one — the shapes where a document says "this font is missing" and the
question is what may lawfully be written into it.

Every file pins ONE branch and is named for it. Written by hand, object by
object, for the same reason the other two generators are: the whole point of
several of these files is a structure a helpful library would normalise away
(a font dictionary with no `/Widths`, two fonts sharing one descriptor, a
`/Subtype` a program cannot lawfully attach to).

Project rule 7 (test-corpus sourcing): every byte is synthetic and
self-authored. The donor programs are the project's own
`fixtures/synthetic/text/subset-fstype-*.ttf` faces from
`tools/gen-subset-font-fixtures.py`.

WHAT EACH FIXTURE PINS
----------------------

| File | Branch |
|---|---|
| `embed-std14-bare.pdf` | `/Helvetica` with **no** `/FontDescriptor`, no `/Widths`, no `/FirstChar`/`/LastChar`, no `/Encoding` — the shape §9.6.2.2 explicitly permits, and **82.9 % of every non-embedded font in the corpus**. Drives `EmbedShape::Synthesise`: pdfcer writes the descriptor, the metrics and the encoding from its compiled Adobe Core-14 data, and re-declares `/Subtype /TrueType` so a `glyf` donor can attach (§9.9 Table 126). |
| `embed-std14-encoded.pdf` | `/Helvetica` bare **except** for an explicit `/Encoding /WinAnsiEncoding`. Same synthesise path, but the encoding is already pinned, so pdfcer must NOT write one — and the `/Widths` it writes must be computed under WinAnsi, not under the built-in Standard encoding. Two encodings disagree on a dozen codes; this is the fixture that catches using the wrong one. |
| `embed-attach.pdf` | A non-embedded `/TrueType` font that already carries a `/FontDescriptor` and `/Widths`. Drives `EmbedShape::Attach`: exactly ONE key is added to ONE descriptor and nothing else changes. |
| `embed-mixed.pdf` | FIVE fonts in one document — one attachable TrueType, one bare standard-14, one `Type0`/`Identity-H` composite, one `Type3`, and one already-embedded font. The partial-success case, which is the COMMON case: some resolve, some refuse by name, and the report must say which is which. |
| `embed-shared-descriptor.pdf` | Two non-embedded font dictionaries with different `/BaseFont` values pointing at ONE `/FontDescriptor`. Both are blocked (`descriptor-shared`). ★ This is where embedding DIVERGES from unembedding: unembedding two fonts through one descriptor is idempotent, embedding two different donors through one descriptor is a silent overwrite. |
| `embed-std14-dingbats.pdf` | `/ZapfDingbats`, bare. Symbolic: its codes mean what its own program says, so an inferred donor is refused (`symbolic-substitute`) and only an exact-name match proceeds. Also unreachable by the `/TrueType` re-declaration — its glyph names (`a1`…`a191`) are not in the Adobe Glyph List, so §9.6.6.4 Branch A could not complete. |
| `embed-symbolic-truetype.pdf` | A **symbolic** `/TrueType` font with metrics and no `/Encoding`. §9.6.6.4 Branch B maps its codes through the *program's own* `(3,0)` cmap, so an inferred donor draws the wrong symbols silently — refused (`symbolic-substitute`). The fixture that proves the guard still fires after it was narrowed to permit the name-mapped case. |
| `embed-xrefstream-outside-size.pdf` | ★ Not an embedding shape. A file whose cross-reference **stream** is object 6 while its own `/Size` is 6 and its `/Index` omits it. `Document::next_object_number` handed out 6 — the number the writer reuses for its own section — so any created object was silently overwritten. Found by `tools/embed-sweep` over the pdfium corpus; the bug predates this Pass and affects every object-creating command. |
| `embed-nometrics.pdf` | A `/Type1` font that is **not** one of the standard 14 and carries neither a descriptor nor `/Widths`. There is no source for the advances the file assumes, so it is refused (`no-metric-source`) rather than embedded with the donor's own widths, which would move every glyph on the page. |

★ WHY `embed-std14-encoded.pdf` EXISTS AS A SEPARATE FILE

`StandardEncoding` and `WinAnsiEncoding` assign different glyphs to the same
code in more than a dozen places (`0o47` is `quoteright` under one and
`quotesingle` under the other). A `/Widths` array computed under the wrong
one is wrong in a way that shows up as *slightly* mis-spaced text on a page
that otherwise looks perfect — the failure mode least likely to be noticed
in a screenshot and most likely to be noticed by a print service. Two
fixtures differing in exactly that one entry is the cheapest way to make
that mistake fail a test instead of shipping.

Usage:  python tools/gen-embed-fixtures.py
"""

import os

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.normpath(os.path.join(HERE, '..'))
TEXT = os.path.join(ROOT, 'fixtures', 'synthetic', 'text')
OUT = os.path.join(ROOT, 'fixtures', 'synthetic', 'embed')

os.makedirs(OUT, exist_ok=True)


def donor(name):
    """Read one of the project's own synthetic sfnt donors."""
    with open(os.path.join(TEXT, f'subset-fstype-{name}.ttf'), 'rb') as f:
        return f.read()


def build(objects, root):
    """Serialise a 1-based list of object bodies into a classic-xref PDF."""
    out = bytearray(b'%PDF-1.7\n%\xe2\xe3\xcf\xd3\n')
    offsets = []
    for i, body in enumerate(objects, start=1):
        offsets.append(len(out))
        out += f'{i} 0 obj\n'.encode('ascii') + body + b'\nendobj\n'
    xref = len(out)
    out += f'xref\n0 {len(objects) + 1}\n'.encode('ascii')
    out += b'0000000000 65535 f \n'
    for off in offsets:
        out += f'{off:010d} 00000 n \n'.encode('ascii')
    out += (
        f'trailer\n<< /Size {len(objects) + 1} /Root {root} 0 R >>\n'
        f'startxref\n{xref}\n'.encode('ascii')
    )
    out += b'%%EOF\n'
    return bytes(out)


def stream(dict_body, payload):
    head = dict_body.rstrip()
    assert head.endswith(b'>>')
    head = head[:-2] + f' /Length {len(payload)} >>'.encode('ascii')
    return head + b'\nstream\n' + payload + b'\nendstream'


def write(name, data):
    path = os.path.join(OUT, name)
    with open(path, 'wb') as f:
        f.write(data)
    print(f'  {name:34s} {len(data):7d} bytes')


DESC_METRICS = (
    b'/Flags 32 /FontBBox [0 -200 600 800] /ItalicAngle 0 /Ascent 800 '
    b'/Descent -200 /CapHeight 700 /StemV 80'
)


# ---------------------------------------------------------------------------
# 1. The 83 % case: a bare standard-14 font.
#
#    §9.6.2.2 lets the 14 standard fonts omit the descriptor and every
#    metric, because a conforming reader is required to know them. A file
#    that leans on that permission has NOTHING in it for pdfcer to attach a
#    program to — the descriptor has to be authored, and the widths with it.
# ---------------------------------------------------------------------------
def std14_bare():
    objs = [
        b'<< /Type /Catalog /Pages 2 0 R >>',
        b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] '
        b'/Resources << /Font << /F0 5 0 R >> >> /Contents 4 0 R >>',
        stream(b'<< >>', b'BT /F0 12 Tf 20 200 Td (Hello) Tj ET\n'),
        b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
    ]
    return build(objs, 1)


# ---------------------------------------------------------------------------
# 2. The same, with the encoding already pinned.
#
#    The widths pdfcer writes must follow THIS encoding, not the standard-14
#    built-in one. See the module docstring for why that is worth a file.
# ---------------------------------------------------------------------------
def std14_encoded():
    objs = [
        b'<< /Type /Catalog /Pages 2 0 R >>',
        b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] '
        b'/Resources << /Font << /F0 5 0 R >> >> /Contents 4 0 R >>',
        stream(b'<< >>', b'BT /F0 12 Tf 20 200 Td (Hello) Tj ET\n'),
        b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica '
        b'/Encoding /WinAnsiEncoding >>',
    ]
    return build(objs, 1)


# ---------------------------------------------------------------------------
# 3. The drop-in case: descriptor and widths already present.
#
#    ONE key is added to object 6 and NOTHING else changes. The round-trip
#    test over this file is the sharpest one in the Pass: the incremental
#    update section must carry exactly the descriptor and the new stream.
# ---------------------------------------------------------------------------
def attach():
    objs = [
        b'<< /Type /Catalog /Pages 2 0 R >>',
        b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] '
        b'/Resources << /Font << /F0 5 0 R >> >> /Contents 4 0 R >>',
        stream(b'<< >>', b'BT /F0 12 Tf 20 200 Td (ABC) Tj ET\n'),
        b'<< /Type /Font /Subtype /TrueType /BaseFont /pdfceMissing '
        b'/FirstChar 65 /LastChar 67 /Widths [600 600 600] '
        b'/Encoding /WinAnsiEncoding /FontDescriptor 6 0 R >>',
        b'<< /Type /FontDescriptor /FontName /pdfceMissing ' + DESC_METRICS + b' >>',
    ]
    return build(objs, 1)


# ---------------------------------------------------------------------------
# 4. Partial success — the COMMON case, and the one an all-or-nothing
#    result would get wrong.
#
#    /F0 attachable TrueType · /F1 bare Helvetica · /F2 Identity-H composite
#    (refused: its codes are glyph indices into the absent program) ·
#    /F3 Type3 (refused: nothing is missing) · /F4 already embedded.
# ---------------------------------------------------------------------------
def mixed():
    prog = donor('editable')
    objs = [
        b'<< /Type /Catalog /Pages 2 0 R >>',
        b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 300] '
        b'/Resources << /Font << /F0 5 0 R /F1 7 0 R /F2 8 0 R /F3 10 0 R '
        b'/F4 12 0 R >> >> /Contents 4 0 R >>',
        stream(
            b'<< >>',
            b'BT /F0 12 Tf 20 260 Td (ABC) Tj /F1 12 Tf 20 220 Td (Hi) Tj '
            b'/F4 12 Tf 20 180 Td (ABC) Tj ET\n',
        ),
        # 5: attachable
        b'<< /Type /Font /Subtype /TrueType /BaseFont /pdfceAttach '
        b'/FirstChar 65 /LastChar 67 /Widths [600 600 600] '
        b'/Encoding /WinAnsiEncoding /FontDescriptor 6 0 R >>',
        b'<< /Type /FontDescriptor /FontName /pdfceAttach ' + DESC_METRICS + b' >>',
        # 7: bare standard-14
        b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
        # 8/9: composite, Identity-H, no program
        b'<< /Type /Font /Subtype /Type0 /BaseFont /pdfceComposite '
        b'/Encoding /Identity-H /DescendantFonts [9 0 R] >>',
        b'<< /Type /Font /Subtype /CIDFontType2 /BaseFont /pdfceComposite '
        b'/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> '
        b'/DW 1000 >>',
        # 10/11: Type 3 — glyphs are content streams already in the file
        b'<< /Type /Font /Subtype /Type3 /FontBBox [0 0 600 800] '
        b'/FontMatrix [0.001 0 0 0.001 0 0] /CharProcs << /a 11 0 R >> '
        b'/Encoding << /Type /Encoding /Differences [97 /a] >> '
        b'/FirstChar 97 /LastChar 97 /Widths [600] >>',
        stream(b'<< >>', b'600 0 0 0 600 800 d1\n'),
        # 12/13/14: already embedded — nothing to do
        b'<< /Type /Font /Subtype /TrueType /BaseFont /pdfceHasProgram '
        b'/FirstChar 65 /LastChar 67 /Widths [600 600 600] '
        b'/Encoding /WinAnsiEncoding /FontDescriptor 13 0 R >>',
        b'<< /Type /FontDescriptor /FontName /pdfceHasProgram ' + DESC_METRICS
        + b' /FontFile2 14 0 R >>',
        stream(b'<< /Length1 %d >>' % len(prog), prog),
    ]
    return build(objs, 1)


# ---------------------------------------------------------------------------
# 5. ★ The divergence from unembedding.
#
#    Two font dictionaries with DIFFERENT /BaseFont values reaching ONE
#    descriptor. Unembedding both is idempotent — the same key is removed
#    twice. Embedding both is not: two names resolve to two donors, and one
#    descriptor can only name one /FontFile2, so whichever ran last would
#    silently give one font the other's letterforms.
#
#    So BOTH are blocked, and this fixture is what proves the mirror module's
#    "outsiders only" rule was NOT copied across unexamined.
# ---------------------------------------------------------------------------
def shared_descriptor():
    objs = [
        b'<< /Type /Catalog /Pages 2 0 R >>',
        b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] '
        b'/Resources << /Font << /F0 5 0 R /F1 7 0 R >> >> /Contents 4 0 R >>',
        stream(b'<< >>', b'BT /F0 12 Tf 20 200 Td (ABC) Tj /F1 12 Tf 20 160 Td (ABC) Tj ET\n'),
        b'<< /Type /Font /Subtype /TrueType /BaseFont /pdfceSharedA '
        b'/FirstChar 65 /LastChar 67 /Widths [600 600 600] '
        b'/Encoding /WinAnsiEncoding /FontDescriptor 6 0 R >>',
        b'<< /Type /FontDescriptor /FontName /pdfceSharedA ' + DESC_METRICS + b' >>',
        b'<< /Type /Font /Subtype /TrueType /BaseFont /pdfceSharedB '
        b'/FirstChar 65 /LastChar 67 /Widths [600 600 600] '
        b'/Encoding /WinAnsiEncoding /FontDescriptor 6 0 R >>',
    ]
    return build(objs, 1)


# ---------------------------------------------------------------------------
# 6. A symbolic standard-14 face.
#
#    ZapfDingbats' codes mean whatever its own program says. An inferred
#    donor draws a different repertoire — wrong symbols, not merely
#    different-looking ones — so only an exact-name match proceeds.
# ---------------------------------------------------------------------------
def std14_dingbats():
    objs = [
        b'<< /Type /Catalog /Pages 2 0 R >>',
        b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] '
        b'/Resources << /Font << /F0 5 0 R >> >> /Contents 4 0 R >>',
        stream(b'<< >>', b'BT /F0 12 Tf 20 200 Td (34) Tj ET\n'),
        b'<< /Type /Font /Subtype /Type1 /BaseFont /ZapfDingbats >>',
    ]
    return build(objs, 1)


# ---------------------------------------------------------------------------
# 7. No metric source at all.
#
#    Not one of the standard 14, and carrying neither a descriptor nor
#    /Widths. Embedding a donor here would take the advances from the DONOR,
#    which is the one thing this operation promises never to do.
# ---------------------------------------------------------------------------
def no_metrics():
    objs = [
        b'<< /Type /Catalog /Pages 2 0 R >>',
        b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] '
        b'/Resources << /Font << /F0 5 0 R >> >> /Contents 4 0 R >>',
        stream(b'<< >>', b'BT /F0 12 Tf 20 200 Td (ABC) Tj ET\n'),
        b'<< /Type /Font /Subtype /Type1 /BaseFont /pdfceNoMetrics >>',
    ]
    return build(objs, 1)


# ---------------------------------------------------------------------------
# 8. A SYMBOLIC TrueType font, with metrics and no /Encoding.
#
#    §9.6.6.4 Branch B: the codes are looked up in the PROGRAM's own (3,0)
#    cmap. A face the document never named draws whatever glyph happens to
#    sit at that code — the wrong symbol, silently. So an inferred donor is
#    refused here, and this fixture is the one that proves the guard still
#    fires after it was narrowed to allow the name-mapped case.
# ---------------------------------------------------------------------------
def symbolic_truetype():
    objs = [
        b'<< /Type /Catalog /Pages 2 0 R >>',
        b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] '
        b'/Resources << /Font << /F0 5 0 R >> >> /Contents 4 0 R >>',
        stream(b'<< >>', b'BT /F0 12 Tf 20 200 Td (ABC) Tj ET\n'),
        # No /Encoding, and the descriptor says Symbolic (Table 123 bit 3).
        b'<< /Type /Font /Subtype /TrueType /BaseFont /pdfceSymbolic '
        b'/FirstChar 65 /LastChar 67 /Widths [600 600 600] /FontDescriptor 6 0 R >>',
        b'<< /Type /FontDescriptor /FontName /pdfceSymbolic /Flags 4 '
        b'/FontBBox [0 -200 600 800] /ItalicAngle 0 /Ascent 800 /Descent -200 '
        b'/CapHeight 700 /StemV 80 >>',
    ]
    return build(objs, 1)


# ---------------------------------------------------------------------------
# 9. ★ THE OBJECT-NUMBER COLLISION. Not an embedding shape at all.
#
#    A cross-reference STREAM is an indirect object, and pdfcer's writer
#    reuses its number for the update section it emits (R33 — match the
#    base's section shape). But it is *the section*, not a body object: the
#    parser never files it, and nothing requires it to appear in its own
#    /Index or to be covered by its own /Size.
#
#    This file is shaped exactly like `pdfium/testing/resources/
#    annotation_stamp_with_ap.pdf`, where `embed-sweep` found the bug: the
#    xref stream is object 6 and its /Size is 6, so every source
#    `Document::next_object_number` consulted answered 5 and it handed out
#    6 — the number the writer was about to spend on its own section.
#
#    The failure was SILENT in the worst way. The file parses, opens and
#    renders; only the created object is gone, overwritten by the new xref
#    stream later in the file. Any command that creates an object hits it,
#    not just embedding.
#
#    Its data is deliberately a CLASSIC-shaped /Index that omits object 6
#    itself, which is what real producers of this shape do.
# ---------------------------------------------------------------------------
def xref_stream_outside_its_own_size():
    import zlib

    buf = bytearray(b'%PDF-1.5\n%\xe2\xe3\xcf\xd3\n')
    off = {}

    def obj(n, body):
        off[n] = len(buf)
        buf.extend(b'%d 0 obj\n' % n + body + b'\nendobj\n')

    content = b'BT /F0 12 Tf 20 200 Td (Hello) Tj ET\n'
    obj(1, b'<< /Type /Catalog /Pages 2 0 R >>')
    obj(2, b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>')
    obj(3, b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] '
           b'/Resources << /Font << /F0 5 0 R >> >> /Contents 4 0 R >>')
    obj(4, b'<< /Length %d >>\nstream\n%s\nendstream' % (len(content), content))
    obj(5, b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>')

    # The xref stream is object 6. Entries for 0..5 only — /Size 6 and an
    # /Index that does not mention 6, exactly as the real file does.
    rows = bytearray()
    rows += bytes([0, 0, 0, 0, 0, 255, 255])          # free head
    for n in range(1, 6):
        rows += bytes([1]) + off[n].to_bytes(4, 'big') + bytes([0, 0])
    data = zlib.compress(bytes(rows))
    off[6] = len(buf)
    buf.extend(
        b'6 0 obj\n<< /Type /XRef /Root 1 0 R /Size 6 /Index [0 6] '
        b'/W [1 4 2] /Filter /FlateDecode /Length %d >>\nstream\n%s\nendstream\nendobj\n'
        % (len(data), data)
    )
    buf.extend(b'startxref\n%d\n%%%%EOF\n' % off[6])
    return bytes(buf)


# ---------------------------------------------------------------------------
# A donor FOLDER, so `--font-dir` can be exercised without depending on the
# machine's own fonts.
#
# `pdfcer`'s font walk registers every face under its advertised names AND
# under its FILENAME STEM (decision 012), which is what makes this work: the
# same synthetic sfnt copied under a name a fixture's /BaseFont spells is a
# face pdfcer will resolve for that font, on any machine, with no system font
# folder involved. A test that pointed --font-dir at C:\Windows\Fonts would
# pass on one laptop and be vacuous everywhere else.
# ---------------------------------------------------------------------------
def donor_folder():
    folder = os.path.join(OUT, 'fonts')
    os.makedirs(folder, exist_ok=True)
    for stem, src in [
        ('pdfceMissing', 'editable'),      # the Attach fixture's /BaseFont
        ('pdfceSharedA', 'editable'),      # the shared-descriptor fixture
        ('pdfceSharedB', 'editable'),
        ('pdfceAttach', 'editable'),       # the mixed fixture's attachable font
        ('pdfceRestricted', 'restricted'),  # fsType says: do not embed
    ]:
        with open(os.path.join(folder, stem + '.ttf'), 'wb') as f:
            f.write(donor(src))
        print(f'  fonts/{stem + ".ttf":28s} {len(donor(src)):7d} bytes')


print('gen-embed-fixtures ->', OUT)
write('embed-std14-bare.pdf', std14_bare())
write('embed-std14-encoded.pdf', std14_encoded())
write('embed-attach.pdf', attach())
write('embed-mixed.pdf', mixed())
write('embed-shared-descriptor.pdf', shared_descriptor())
write('embed-std14-dingbats.pdf', std14_dingbats())
write('embed-nometrics.pdf', no_metrics())
write('embed-symbolic-truetype.pdf', symbolic_truetype())
write('embed-xrefstream-outside-size.pdf', xref_stream_outside_its_own_size())
donor_folder()
