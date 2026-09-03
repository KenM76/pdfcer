"""Generate the font-UNEMBEDDING fixtures for `pdfcer_core::font_unembed`
(Pass 67.0 phase B).

Phase A's `tools/gen-fontinfo-fixtures.py` covers the CLASSIFIER — one file
per removability verdict. This generator covers what the classifier cannot
reach: the STRUCTURAL hazards that only matter once pdfcer starts deleting
objects, and the disclosures that only matter once it does.

Every file here exists to pin ONE branch, and each is named for that branch.
Written by hand, object by object, for the same reason phase A's are: a
library that "helpfully" normalised a descriptor would silently defeat the
test that reads it — and here the whole point is that two font dictionaries
really do share one object.

Project rule 7 (test-corpus sourcing): every byte is synthetic and
self-authored. The embedded programs are the project's own
`fixtures/synthetic/text/subset-fstype-*.ttf` donors from
`tools/gen-subset-font-fixtures.py`.

WHAT EACH FIXTURE PINS
----------------------

| File | Branch |
|---|---|
| `unembed-shared-program.pdf` | Two descriptors, ONE `/FontFile2` object. The removable font unembeds; the stream is **not freed** because a blocked font still reaches it. Deleting it would blank that font — the failure this fixture exists to make impossible. |
| `unembed-shared-descriptor.pdf` | Two font dictionaries, ONE `/FontDescriptor` object. The removable font is **blocked outright** (`descriptor-shared`): editing that descriptor would unembed a font pdfcer refused to touch. |
| `unembed-many-pages.pdf` | One font object reached from five pages. Deduplicated to one target listing five pages, and one deletion — not five. |
| `unembed-acroform-dr.pdf` | A removable embedded font reachable ONLY from the AcroForm `/DR /Font` default-resource dictionary (§12.7.2). It has no page, so its `pages` list is empty and the operation must still work. |
| `unembed-charset-cidset.pdf` | A descriptor carrying BOTH `/CharSet` (Table 122) and `/CIDSet` (Table 124, deliberately malformed on a simple font — see below). Both go with the program. |
| `unembed-direct-fontdict.pdf` | The font dictionary is written INLINE in the page's `/Resources`. It has no object identity, so it is blocked by name (`font-not-indirect`) rather than silently skipped. |
| `unembed-inline-descriptor.pdf` | The `/FontDescriptor` is a direct dictionary inside an indirect font dictionary. Addressable — writing the font dictionary writes it — so it unembeds, and the test proves the inline path is not confused with the shared-object one. |
| `unembed-pdfa.pdf` | XMP `pdfaid:part`/`pdfaid:conformance` in the catalog `/Metadata`. Unembedding breaks the conformance the file claims, and the plan must say so BEFORE anything happens. |
| `unembed-shortest-subset-tag.pdf` | `/BaseFont /ABCDEF+X` — the SHORTEST name §9.6.4 can tag. Measured, not assumed: `split_subset_tag` requires more than seven bytes, so a bare `ABCDEF+` is **not** a tag at all and a tagged name always leaves at least one character behind. |

★ WHY `/CIDSet` APPEARS ON A SIMPLE FONT, DELIBERATELY

Table 124 puts `/CIDSet` on a CIDFont descriptor, and phase A's classifier
never returns `Removable` for a composite font — so a CONFORMING document
cannot present "removable font whose descriptor carries `/CIDSet`". The
handling exists anyway (a descriptor carrying it is asserting "this used to
be a subset", which is exactly the false claim unembedding must not leave
behind), and code that cannot be reached by any test is code nobody has
checked. The fixture is malformed ON PURPOSE and says so here, rather than
the branch going untested because the legal shape does not exist.

Usage:  python tools/gen-unembed-fixtures.py
"""

import os
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.normpath(os.path.join(HERE, '..'))
TEXT = os.path.join(ROOT, 'fixtures', 'synthetic', 'text')
OUT = os.path.join(ROOT, 'fixtures', 'synthetic', 'unembed')

os.makedirs(OUT, exist_ok=True)


def donor(name):
    """Read one of the project's own synthetic sfnt donors."""
    with open(os.path.join(TEXT, f'subset-fstype-{name}.ttf'), 'rb') as f:
        return f.read()


def build(objects, root):
    """Serialise a 1-based list of object bodies into a classic-xref PDF.

    Identical in shape to `gen-fontinfo-fixtures.py`'s: plain §7.5.4 xref
    table, no xref streams, no object streams — a fixture that needs a
    compressed-object parser to be read is testing two things at once.
    """
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


def stream(dict_body, payload, compress=False):
    """A stream object: dictionary text plus payload, `/Length` correct."""
    if compress:
        payload = zlib.compress(payload)
        dict_body = dict_body.rstrip()[:-2] + b' /Filter /FlateDecode >>'
    head = dict_body.rstrip()
    assert head.endswith(b'>>')
    head = head[:-2] + f' /Length {len(payload)} >>'.encode('ascii')
    return head + b'\nstream\n' + payload + b'\nendstream'


def write(name, data):
    path = os.path.join(OUT, name)
    with open(path, 'wb') as f:
        f.write(data)
    print(f'  {name:36s} {len(data):7d} bytes')


# The descriptor keys every fixture below shares. Written once so a change
# to the metrics cannot make two fixtures disagree about the same face.
DESC_METRICS = (
    b'/Flags 32 /FontBBox [0 -200 600 800] /ItalicAngle 0 /Ascent 800 '
    b'/Descent -200 /CapHeight 700 /StemV 80'
)

SIMPLE_FONT_TAIL = (
    b'/FirstChar 65 /LastChar 67 /Widths [600 600 600] /Encoding /WinAnsiEncoding'
)


# ---------------------------------------------------------------------------
# 1. Two descriptors, ONE font-program object.
#
#    Font /F0 is a removable simple font; /F1 is a symbolic TrueType with no
#    /Encoding, which phase A classifies Unknown(SymbolicBuiltinEncoding).
#    Both descriptors name object 9 as their /FontFile2.
#
#    Legal (§7.3.10 puts no restriction on how many references an object
#    may have) and the exact shape that turns a naive "delete the stream"
#    into a silently blanked font. /F0 must unembed; object 9 must SURVIVE.
# ---------------------------------------------------------------------------
def shared_program():
    objs = [
        b'<< /Type /Catalog /Pages 2 0 R >>',
        b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] '
        b'/Resources << /Font << /F0 5 0 R /F1 6 0 R >> >> /Contents 4 0 R >>',
        stream(b'<< >>', b'BT /F0 12 Tf 20 200 Td (ABC) Tj /F1 12 Tf 20 160 Td (ABC) Tj ET\n'),
        # Removable: standard base encoding.
        b'<< /Type /Font /Subtype /TrueType /BaseFont /AAAAAA+pdfceShared '
        + SIMPLE_FONT_TAIL + b' /FontDescriptor 7 0 R >>',
        # Symbolic, no /Encoding => Unknown(SymbolicBuiltinEncoding).
        b'<< /Type /Font /Subtype /TrueType /BaseFont /BBBBBB+pdfceShared '
        b'/FirstChar 65 /LastChar 67 /Widths [600 600 600] /FontDescriptor 8 0 R >>',
        b'<< /Type /FontDescriptor /FontName /AAAAAA+pdfceShared ' + DESC_METRICS
        + b' /FontFile2 9 0 R >>',
        # Flags 4 = Symbolic (§9.8.2 Table 123 bit 3).
        b'<< /Type /FontDescriptor /FontName /BBBBBB+pdfceShared /Flags 4 '
        b'/FontBBox [0 -200 600 800] /ItalicAngle 0 /Ascent 800 /Descent -200 '
        b'/CapHeight 700 /StemV 80 /FontFile2 9 0 R >>',
        stream(b'<< /Length1 %d >>' % len(donor('editable')), donor('editable')),
    ]
    return build(objs, 1)


# ---------------------------------------------------------------------------
# 2. Two font dictionaries, ONE descriptor object.
#
#    /F0 is removable, /F1 is symbolic-with-no-encoding — and both name
#    object 6 as their /FontDescriptor. Removing /FontFile2 from object 6
#    unembeds BOTH, including the one pdfcer explicitly refused.
#
#    So /F0 is blocked, by name, with `descriptor-shared`. That is the
#    refusal this fixture exists to prove fires; without it the operation
#    would look correct and would silently break a blocked font.
# ---------------------------------------------------------------------------
def shared_descriptor():
    objs = [
        b'<< /Type /Catalog /Pages 2 0 R >>',
        b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] '
        b'/Resources << /Font << /F0 5 0 R /F1 8 0 R >> >> /Contents 4 0 R >>',
        stream(b'<< >>', b'BT /F0 12 Tf 20 200 Td (ABC) Tj /F1 12 Tf 20 160 Td (ABC) Tj ET\n'),
        b'<< /Type /Font /Subtype /TrueType /BaseFont /CCCCCC+pdfceOneDesc '
        + SIMPLE_FONT_TAIL + b' /FontDescriptor 6 0 R >>',
        # Symbolic flag (4) on the SHARED descriptor, so /F1 classifies
        # Unknown(SymbolicBuiltinEncoding) while /F0 stays Removable — the
        # encoding is what decides /F0, and it lives on the font dict.
        b'<< /Type /FontDescriptor /FontName /CCCCCC+pdfceOneDesc /Flags 4 '
        b'/FontBBox [0 -200 600 800] /ItalicAngle 0 /Ascent 800 /Descent -200 '
        b'/CapHeight 700 /StemV 80 /FontFile2 7 0 R >>',
        stream(b'<< /Length1 %d >>' % len(donor('editable')), donor('editable')),
        b'<< /Type /Font /Subtype /TrueType /BaseFont /CCCCCC+pdfceOneDesc '
        b'/FirstChar 65 /LastChar 67 /Widths [600 600 600] /FontDescriptor 6 0 R >>',
    ]
    return build(objs, 1)


# ---------------------------------------------------------------------------
# 3. One font object, five pages.
#
#    Phase A deduplicates by object identity, so this must be ONE target
#    listing five pages. A per-page loop would delete the same object five
#    times and count its bytes five times — an operation that reported five
#    times the truth about what it recovered.
# ---------------------------------------------------------------------------
def many_pages():
    page_ids = [4, 5, 6, 7, 8]
    kids = b' '.join(f'{n} 0 R'.encode('ascii') for n in page_ids)
    objs = [
        b'<< /Type /Catalog /Pages 2 0 R >>',
        b'<< /Type /Pages /Kids [' + kids + b'] /Count 5 /MediaBox [0 0 300 300] '
        b'/Resources << /Font << /F0 9 0 R >> >> >>',
        stream(b'<< >>', b'BT /F0 12 Tf 20 200 Td (ABC) Tj ET\n'),
    ]
    for _ in page_ids:
        objs.append(b'<< /Type /Page /Parent 2 0 R /Contents 3 0 R >>')
    objs.append(
        b'<< /Type /Font /Subtype /TrueType /BaseFont /DDDDDD+pdfceManyPages '
        + SIMPLE_FONT_TAIL + b' /FontDescriptor 10 0 R >>'
    )
    objs.append(
        b'<< /Type /FontDescriptor /FontName /DDDDDD+pdfceManyPages ' + DESC_METRICS
        + b' /FontFile2 11 0 R >>'
    )
    objs.append(stream(b'<< /Length1 %d >>' % len(donor('editable')), donor('editable')))
    return build(objs, 1)


# ---------------------------------------------------------------------------
# 4. A removable embedded font reachable ONLY from the AcroForm /DR /Font.
#
#    §12.7.2 Table 218: `/DR` is "a resource dictionary containing default
#    resources … that shall be used by form field appearance streams". The
#    font is named from no page, so its `pages` list is empty — and an
#    implementation that keyed deletion off pages would find nothing to do
#    while reporting a font it could remove.
# ---------------------------------------------------------------------------
def acroform_dr():
    objs = [
        b'<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [] '
        b'/DR << /Font << /Helv 5 0 R >> >> /DA (/Helv 0 Tf 0 g) >> >>',
        b'<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 300 300] >>',
        b'<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>',
        stream(b'<< >>', b'0 0 1 rg 10 10 50 50 re f\n'),
        b'<< /Type /Font /Subtype /TrueType /BaseFont /EEEEEE+pdfceFormFont '
        + SIMPLE_FONT_TAIL + b' /FontDescriptor 6 0 R >>',
        b'<< /Type /FontDescriptor /FontName /EEEEEE+pdfceFormFont ' + DESC_METRICS
        + b' /FontFile2 7 0 R >>',
        stream(b'<< /Length1 %d >>' % len(donor('editable')), donor('editable')),
    ]
    return build(objs, 1)


# ---------------------------------------------------------------------------
# 5. A descriptor carrying BOTH /CharSet and /CIDSet.
#
#    `/CIDSet` on a simple font is malformed (Table 124 puts it on a CIDFont
#    descriptor) and is here ON PURPOSE — see this file's header. Both
#    entries describe the glyph coverage of the program being deleted, so
#    both go with it, and the /CIDSet STREAM is freed alongside.
# ---------------------------------------------------------------------------
def charset_cidset():
    objs = [
        b'<< /Type /Catalog /Pages 2 0 R >>',
        b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] '
        b'/Resources << /Font << /F0 5 0 R >> >> /Contents 4 0 R >>',
        stream(b'<< >>', b'BT /F0 12 Tf 20 200 Td (ABC) Tj ET\n'),
        b'<< /Type /Font /Subtype /TrueType /BaseFont /FFFFFF+pdfceCoverage '
        + SIMPLE_FONT_TAIL + b' /FontDescriptor 6 0 R >>',
        b'<< /Type /FontDescriptor /FontName /FFFFFF+pdfceCoverage ' + DESC_METRICS
        + b' /CharSet (/A/B/C) /CIDSet 8 0 R /FontFile2 7 0 R >>',
        stream(b'<< /Length1 %d >>' % len(donor('editable')), donor('editable')),
        # §9.8.3: bits indexed by CID, high-order bit first. Three glyphs.
        stream(b'<< >>', bytes([0b11100000])),
    ]
    return build(objs, 1)


# ---------------------------------------------------------------------------
# 6. The font dictionary written INLINE in the page's /Resources.
#
#    §7.3.7 permits a direct dictionary anywhere a dictionary is allowed, so
#    this is legal and rare. It has no object identity, so the overlay has
#    nothing to write — blocked by name (`font-not-indirect`), never silently
#    absent from both the removed and the refused list.
# ---------------------------------------------------------------------------
def direct_fontdict():
    objs = [
        b'<< /Type /Catalog /Pages 2 0 R >>',
        b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 4 0 R '
        b'/Resources << /Font << /F0 << /Type /Font /Subtype /TrueType '
        b'/BaseFont /GGGGGG+pdfceInlineFont ' + SIMPLE_FONT_TAIL
        + b' /FontDescriptor 5 0 R >> >> >> >>',
        stream(b'<< >>', b'BT /F0 12 Tf 20 200 Td (ABC) Tj ET\n'),
        b'<< /Type /FontDescriptor /FontName /GGGGGG+pdfceInlineFont ' + DESC_METRICS
        + b' /FontFile2 6 0 R >>',
        stream(b'<< /Length1 %d >>' % len(donor('editable')), donor('editable')),
    ]
    return build(objs, 1)


# ---------------------------------------------------------------------------
# 7. The /FontDescriptor written INLINE in an indirect font dictionary.
#
#    Addressable — writing object 5 writes the descriptor with it — so this
#    UNEMBEDS. Its counterpart in `direct_fontdict` does not, and the pair
#    is what proves the two "direct" cases are not being confused: one has
#    an object to write, the other does not.
# ---------------------------------------------------------------------------
def inline_descriptor():
    objs = [
        b'<< /Type /Catalog /Pages 2 0 R >>',
        b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] '
        b'/Resources << /Font << /F0 5 0 R >> >> /Contents 4 0 R >>',
        stream(b'<< >>', b'BT /F0 12 Tf 20 200 Td (ABC) Tj ET\n'),
        b'<< /Type /Font /Subtype /TrueType /BaseFont /HHHHHH+pdfceInlineDesc '
        + SIMPLE_FONT_TAIL
        + b' /FontDescriptor << /Type /FontDescriptor '
        b'/FontName /HHHHHH+pdfceInlineDesc ' + DESC_METRICS + b' /FontFile2 6 0 R >> >>',
        stream(b'<< /Length1 %d >>' % len(donor('editable')), donor('editable')),
    ]
    return build(objs, 1)


# ---------------------------------------------------------------------------
# 8. A PDF/A-identified document with a removable embedded font.
#
#    PDF/A requires embedded fonts in every part, so unembedding breaks the
#    conformance this file claims. The claim is in the catalog /Metadata XMP
#    packet as `pdfaid:part` / `pdfaid:conformance` — the identification
#    every part of ISO 19005 requires — and an /OutputIntent is included
#    because real PDF/A files carry one.
# ---------------------------------------------------------------------------
def pdfa():
    xmp = (
        b'<?xpacket begin="\xef\xbb\xbf" id="W5M0MpCehiHzreSzNTczkc9d"?>\n'
        b'<x:xmpmeta xmlns:x="adobe:ns:meta/">\n'
        b' <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">\n'
        b'  <rdf:Description rdf:about=""\n'
        b'    xmlns:pdfaid="http://www.aiim.org/pdfa/ns/id/"\n'
        b'    pdfaid:part="2"\n'
        b'    pdfaid:conformance="B"/>\n'
        b' </rdf:RDF>\n'
        b'</x:xmpmeta>\n'
        b'<?xpacket end="w"?>\n'
    )
    objs = [
        b'<< /Type /Catalog /Pages 2 0 R /Metadata 8 0 R '
        b'/OutputIntents [<< /Type /OutputIntent /S /GTS_PDFA1 '
        b'/OutputConditionIdentifier (sRGB) >>] >>',
        b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] '
        b'/Resources << /Font << /F0 5 0 R >> >> /Contents 4 0 R >>',
        stream(b'<< >>', b'BT /F0 12 Tf 20 200 Td (ABC) Tj ET\n'),
        b'<< /Type /Font /Subtype /TrueType /BaseFont /IIIIII+pdfceConformant '
        + SIMPLE_FONT_TAIL + b' /FontDescriptor 6 0 R >>',
        b'<< /Type /FontDescriptor /FontName /IIIIII+pdfceConformant ' + DESC_METRICS
        + b' /FontFile2 7 0 R >>',
        stream(b'<< /Length1 %d >>' % len(donor('editable')), donor('editable')),
        stream(b'<< /Type /Metadata /Subtype /XML >>', xmp),
    ]
    return build(objs, 1)


# ---------------------------------------------------------------------------
# 9. `/BaseFont /ABCDEF+X` — the shortest name §9.6.4 can tag.
#
#    ★ A MEASUREMENT, not an assumption. The plan's rename declines on an
#    empty family part, and this fixture was written to reach that branch —
#    it does not. `split_subset_tag` requires `len() > 7`, so a bare
#    `ABCDEF+` carries NO tag by that function's own strictness and the
#    whole string stays the name. A tagged name therefore always leaves at
#    least one character behind, and the empty-family guard is unreachable
#    by construction.
#
#    The fixture is kept, renamed for what it actually pins: the boundary
#    case where stripping leaves a one-character family. `ABCDEF+X` becomes
#    `X`, which is a real rename and not a decline.
# ---------------------------------------------------------------------------
def shortest_subset_tag():
    objs = [
        b'<< /Type /Catalog /Pages 2 0 R >>',
        b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] '
        b'/Resources << /Font << /F0 5 0 R >> >> /Contents 4 0 R >>',
        stream(b'<< >>', b'BT /F0 12 Tf 20 200 Td (ABC) Tj ET\n'),
        b'<< /Type /Font /Subtype /TrueType /BaseFont /ABCDEF+X '
        + SIMPLE_FONT_TAIL + b' /FontDescriptor 6 0 R >>',
        b'<< /Type /FontDescriptor /FontName /ABCDEF+X ' + DESC_METRICS
        + b' /FontFile2 7 0 R >>',
        stream(b'<< /Length1 %d >>' % len(donor('editable')), donor('editable')),
    ]
    return build(objs, 1)


def main():
    print(f'font-unembed fixtures -> {OUT}')
    write('unembed-shared-program.pdf', shared_program())
    write('unembed-shared-descriptor.pdf', shared_descriptor())
    write('unembed-many-pages.pdf', many_pages())
    write('unembed-acroform-dr.pdf', acroform_dr())
    write('unembed-charset-cidset.pdf', charset_cidset())
    write('unembed-direct-fontdict.pdf', direct_fontdict())
    write('unembed-inline-descriptor.pdf', inline_descriptor())
    write('unembed-pdfa.pdf', pdfa())
    write('unembed-shortest-subset-tag.pdf', shortest_subset_tag())


if __name__ == '__main__':
    main()
