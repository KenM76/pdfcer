"""Generate the two fixtures `Pass 167.0`'s field clipboard is tested against.

WHY THESE FIXTURES EXIST
========================
`demo-form.pdf` and `radio-group-form.pdf` were enough to build field
CREATION, because creation chooses every value itself: it writes `/Helv 0 Tf
0 g`, left quadding, a black `/MK /BC` and no `/AA`, so a fixture carrying
those same defaults can verify it.

A field COPY is the opposite problem. It is judged entirely on properties
pdfcer did NOT choose, and every one of them is invisible when the fixture's
value happens to equal the authoring default:

  * a `/DA` of `/Helv 0 Tf 0 g` cannot show that the font, size and colour
    travelled -- it is exactly what a re-author would have written anyway;
  * a `/Q` of 0 cannot show that quadding travelled;
  * a `/MK /BC` of black cannot show that the border colour travelled --
    `add_text_field` hard-writes black;
  * a `/DA` naming `/Helv` cannot show that the `/DR` font travelled, because
    `ensure_default_resources` puts `/Helv` in every destination anyway.

So every value here is deliberately NOT the default. A test written against a
default-valued fixture would pass against an implementation that carried
nothing at all, which is the specific failure this pair of files removes.

WHAT IT BUILDS
==============

`rich-field-form.pdf` -- one 400x300 page, one merged (Shape A) text field
`TitleBlock.Revision` under a grouping node `TitleBlock`, carrying:

  | key | value | what it catches |
  |---|---|---|
  | `/DA` | `/TB 14 Tf 0 0 1 rg` | font, size AND colour, none of them the default; and a font resource name that is NOT `/Helv` |
  | `/DR /Font /TB` | Helvetica-Bold | the carried font: a destination that lacks `/TB` must gain it, or the `/DA` does not resolve (SS12.7.3.3) |
  | `/Q` | 1 (centred) | quadding |
  | `/MaxLen` | 12 | the length limit |
  | `/DV` | `(A)` | the reset target, which travels even when the value does not |
  | `/V` | `(C)` | the value, which does NOT travel unless asked |
  | `/TU` | `(Revision letter)` | the accessibility name, and R105's "carry" answer |
  | `/Ff` | 4194304 | `DoNotSpellCheck` (bit 23) -- a flag NO `New*Field` spec can express, so it can only arrive by being carried |
  | `/MK /BC` | `[0 0 1]` blue | the reported "a blue-bordered field pastes black" |
  | `/MK /BG` | `[1 1 0.8]` cream | the background colour, which nothing in pdfcer authors |
  | `/BS` | `/S /D /W 2` | a dashed 2pt border, neither of them the default |
  | `/AP /N` | a real stream | the baked appearance |
  | `/AA /C` + `/AA /F` | JavaScript streams | the calculate action, which obliges `/AcroForm /CO` (Table 218) |
  | `/AcroForm /CO` | `[10 0 R]` | so the destination's `/CO` growing by one is measurable against a source that had one |

`rival-font-form.pdf` -- one 400x300 page and an `/AcroForm` whose
`/DR /Font /TB` is **Courier**, not Helvetica-Bold. It exists for exactly one
test: pasting the rich field here must NOT overwrite the destination's own
`/TB`, must install the carried font under a free name, and must rewrite the
pasted field's `/DA` to name it. Without a fixture whose `/TB` DISAGREES, a
paste that silently clobbered the destination's font would pass.

LEGAL
=====
`LEGAL.md` SS5 category (a): wholly synthetic, byte-authored by this script,
no third-party source, no attribution owed or claimed. Classic SS7.5.4
cross-reference table, no encryption, no object streams, no embedded font
programs (the standard-14 `/DR` entries are the bare SS9.6.2.1 four-key form).

Run from the repository root:  `python tools/gen-form-clipboard-fixtures.py`
"""


def stream_obj(dict_prefix, content):
    """A stream object with a correct `/Length`, matching the sibling scripts."""
    return (
        dict_prefix
        + b" /Length %d >>\nstream\n" % len(content)
        + content
        + b"\nendstream"
    )


def build(objs, media_box):
    """Assemble a one-page document with a classic cross-reference table."""
    buf = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n"
    off = {}
    for n in sorted(objs):
        off[n] = len(buf)
        buf += b"%d 0 obj\n" % n + objs[n] + b"\nendobj\n"
    xref_at = len(buf)
    size = max(objs) + 1
    buf += b"xref\n0 %d\n0000000000 65535 f \n" % size
    for n in range(1, size):
        if n in off:
            buf += b"%010d 00000 n \n" % off[n]
        else:
            buf += b"0000000000 65535 f \n"
    buf += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (
        size,
        xref_at,
    )
    assert media_box  # documented, not enforced -- kept for readability
    return buf


# ---------------------------------------------------------------------------
# rich-field-form.pdf
# ---------------------------------------------------------------------------

rich = {}

# The AcroForm's own /DA and /DR. `/TB` is the interesting entry: the field's
# /DA names it, and no destination has it, so it must travel with the clip.
# `/Helv` is present too because `ensure_default_resources` expects to find or
# create it -- keeping it here makes the DIFFERENCE between the two the thing
# under test rather than a missing baseline.
rich[1] = (
    b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [9 0 R] "
    b"/CO [10 0 R] "
    b"/DA (/Helv 0 Tf 0 g) /DR << /Font << "
    b"/Helv << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> "
    b"/TB << /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >> "
    b">> >> >> >>"
)
rich[2] = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"
rich[3] = (
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 300] /Resources << >> "
    b"/Annots [10 0 R] >>"
)

# The baked appearance. Real content, so a paste that dropped the /AP would be
# visible as an empty box rather than as an identical one.
rich[4] = stream_obj(
    b"<< /Type /XObject /Subtype /Form /BBox [0 0 160 24] "
    b"/Resources << /Font << /TB 8 0 R >> >>",
    b"/Tx BMC q BT /TB 14 Tf 0 0 1 rg 2 7 Td (C) Tj ET Q EMC",
)

# The JavaScript the /AA carries. pdfcer recognises and round-trips these and
# NEVER executes them (decision 008 SS5.1, NF4) -- they are here so the
# clipboard's carry/drop decision has something real to carry or drop.
rich[5] = stream_obj(
    b"<<", b"event.value = AFSimple_Calculate('SUM', new Array('Revision'));"
)
rich[6] = stream_obj(b"<<", b"AFNumber_Format(0, 0, 0, 0, '', true);")

rich[8] = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>"

# The grouping node, so the fixture also exercises a DOTTED name -- a paste
# that split the path wrongly would create `TitleBlock.Revision` as one flat
# /T and the FQN would come back doubled.
rich[9] = b"<< /T (TitleBlock) /Kids [10 0 R] >>"

# THE FIELD. Merged (Shape A, SS12.5.6.19): one dictionary that is both the
# field and its sole widget, which is the shape every `add_*_field` writes and
# therefore the shape a paste must reproduce.
rich[10] = (
    b"<< /Type /Annot /Subtype /Widget /FT /Tx /Parent 9 0 R /T (Revision) "
    b"/TU (Revision letter) "
    b"/Ff 4194304 "
    b"/V (C) /DV (A) "
    b"/DA (/TB 14 Tf 0 0 1 rg) /Q 1 /MaxLen 12 "
    b"/Rect [20 240 180 264] /P 3 0 R /F 4 "
    b"/MK << /BC [0 0 1] /BG [1 1 0.8] >> "
    b"/BS << /S /D /W 2 >> "
    b"/AA << /C << /S /JavaScript /JS 5 0 R >> /F << /S /JavaScript /JS 6 0 R >> >> "
    b"/AP << /N 4 0 R >> >>"
)

# ---------------------------------------------------------------------------
# rival-font-form.pdf
# ---------------------------------------------------------------------------

rival = {}
rival[1] = (
    b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [] "
    b"/DA (/Helv 0 Tf 0 g) /DR << /Font << "
    b"/Helv << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> "
    b"/TB << /Type /Font /Subtype /Type1 /BaseFont /Courier >> "
    b">> >> >> >>"
)
rival[2] = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"
rival[3] = (
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 300] /Resources << >> >>"
)


for path, objs in (
    ("fixtures/synthetic/forms/rich-field-form.pdf", rich),
    ("fixtures/synthetic/forms/rival-font-form.pdf", rival),
):
    data = build(objs, True)
    open(path, "wb").write(data)
    print("wrote", path, len(data), "bytes")
