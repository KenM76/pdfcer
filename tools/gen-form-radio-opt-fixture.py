"""Generate `fixtures/synthetic/forms/radio-opt-form.pdf`.

Two radio groups for pasting another member into a group (ISO 32000-1
§12.7.4.2.3, §12.7.4.2.4):

  1. `Size`  -- carries `/Opt [(Small) (Large)]`, kid on-states `/0` and `/1`
     (indices into `/Opt`). A pasted member cannot take a fresh state without
     a matching `/Opt` entry, so this group is the refusal case.
  2. `Shade` -- no `/Opt`; its kid's `/AP /N` is an INDIRECT dictionary
     (object 7) and it also carries an inline `/D`, so renaming a copied
     on-state has to reach both an indirect and an inline sub-dictionary.

Run from the repository root:  `python tools/gen-form-radio-opt-fixture.py`
"""


def stream_obj(dict_prefix, content):
    return (
        dict_prefix
        + b" /Length %d >>\nstream\n" % len(content)
        + content
        + b"\nendstream"
    )


objs = {}
objs[1] = (
    b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [10 0 R 20 0 R] "
    b"/DA (/Helv 0 Tf 0 g) >> >>"
)
objs[2] = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"
objs[3] = (
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << >> "
    b"/Annots [11 0 R 12 0 R 21 0 R] >>"
)
objs[4] = stream_obj(
    b"<< /Type /XObject /Subtype /Form /BBox [0 0 12 12]", b"3 3 6 6 re f"
)
objs[5] = stream_obj(
    b"<< /Type /XObject /Subtype /Form /BBox [0 0 12 12]", b"0.5 0.5 11 11 re S"
)
objs[7] = b"<< /Dark 4 0 R /Off 5 0 R >>"

objs[10] = (
    b"<< /FT /Btn /Ff 32768 /T (Size) /TU (Size) /V /Off "
    b"/Opt [(Small) (Large)] /Kids [11 0 R 12 0 R] >>"
)
for n, state, x in ((11, b"0", 20), (12, b"1", 60)):
    objs[n] = (
        b"<< /Parent 10 0 R /Subtype /Widget /Rect [%d 150 %d 162] /P 3 0 R "
        b"/AS /Off /AP << /N << /%s 4 0 R /Off 5 0 R >> >> >>" % (x, x + 12, state)
    )

objs[20] = b"<< /FT /Btn /Ff 32768 /T (Shade) /TU (Shade) /V /Dark /Kids [21 0 R] >>"
objs[21] = (
    b"<< /Parent 20 0 R /Subtype /Widget /Rect [20 100 32 112] /P 3 0 R "
    b"/AS /Dark /AP << /N 7 0 R /D << /Dark 4 0 R /Off 5 0 R >> >> >>"
)

buf = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n"
off = {}
for n in sorted(objs):
    off[n] = len(buf)
    buf += b"%d 0 obj\n" % n + objs[n] + b"\nendobj\n"
xref_at = len(buf)
size = max(objs) + 1
buf += b"xref\n0 %d\n0000000000 65535 f \n" % size
for n in range(1, size):
    buf += b"%010d 00000 n \n" % off[n] if n in off else b"0000000000 65535 f \n"
buf += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (size, xref_at)

out = "fixtures/synthetic/forms/radio-opt-form.pdf"
open(out, "wb").write(buf)
print("wrote", out, len(buf), "bytes")
