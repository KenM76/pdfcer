"""Generate `fixtures/synthetic/forms/radio-regroup-form.pdf`.

Radio groups in both dictionary shapes (ISO 32000-1 §12.7.3.1, §12.7.4.2.3)
for moving a member from one group to another:

  1. `Left`  -- kids 11 (`/Red`, selected, `/V /Red`) and 12 (`/Blue`).
  2. `Right` -- kids 31 (`/Red`) and 32 (`/Green`, selected). Kids 11 and 31
     SHARE one indirect `/AP /N` dictionary (object 7), so renaming a moved
     member's state must not rename it for the member that stays.
  3. `Solo`  -- one merged field+widget (object 40), state `/Yes`, selected.
  4. `Group.Lone` -- a merged radio (object 50, `/Red`) that is the only kid
     of grouping node `Group` (object 60), so moving it empties `Group`.
  5. `Agree` -- a check box (object 70), the not-a-radio refusal case.

Run from the repository root:  `python tools/gen-form-radio-regroup-fixture.py`
"""


def stream_obj(dict_prefix, content):
    return (
        dict_prefix
        + b" /Length %d >>\nstream\n" % len(content)
        + content
        + b"\nendstream"
    )


RADIO = b"/FT /Btn /Ff 49152"
objs = {}
objs[1] = (
    b"<< /Type /Catalog /Pages 2 0 R /AcroForm << "
    b"/Fields [10 0 R 30 0 R 40 0 R 60 0 R 70 0 R] /DA (/Helv 0 Tf 0 g) >> >>"
)
objs[2] = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"
objs[3] = (
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << >> "
    b"/Annots [11 0 R 12 0 R 31 0 R 32 0 R 40 0 R 50 0 R 70 0 R] >>"
)
objs[4] = stream_obj(
    b"<< /Type /XObject /Subtype /Form /BBox [0 0 12 12]", b"3 3 6 6 re f"
)
objs[5] = stream_obj(
    b"<< /Type /XObject /Subtype /Form /BBox [0 0 12 12]", b"0.5 0.5 11 11 re S"
)
objs[7] = b"<< /Red 4 0 R /Off 5 0 R >>"


def kid(parent, x, y, state, shown, normal):
    return (
        b"<< /Type /Annot /Parent %d 0 R /Subtype /Widget /Rect [%d %d %d %d] "
        b"/P 3 0 R /AS /%s /AP << /N %s >> >>"
        % (parent, x, y, x + 12, y + 12, shown, normal)
    )


def inline(state):
    return b"<< /%s 4 0 R /Off 5 0 R >>" % state


objs[10] = b"<< %s /T (Left) /V /Red /Kids [11 0 R 12 0 R] >>" % RADIO
objs[11] = kid(10, 20, 150, b"Red", b"Red", b"7 0 R")
objs[12] = kid(10, 40, 150, b"Blue", b"Off", inline(b"Blue"))
objs[30] = b"<< %s /T (Right) /V /Green /Kids [31 0 R 32 0 R] >>" % RADIO
objs[31] = kid(30, 20, 120, b"Red", b"Off", b"7 0 R")
objs[32] = kid(30, 40, 120, b"Green", b"Green", inline(b"Green"))
objs[40] = (
    b"<< /Type /Annot /Subtype /Widget %s /T (Solo) /V /Yes /Rect [20 90 32 102] "
    b"/P 3 0 R /AS /Yes /AP << /N %s >> >>" % (RADIO, inline(b"Yes"))
)
objs[50] = (
    b"<< /Type /Annot /Subtype /Widget %s /T (Lone) /Parent 60 0 R /V /Off "
    b"/Rect [20 60 32 72] /P 3 0 R /AS /Off /AP << /N %s >> >>" % (RADIO, inline(b"Red"))
)
objs[60] = b"<< /T (Group) /Kids [50 0 R] >>"
objs[70] = (
    b"<< /Type /Annot /Subtype /Widget /FT /Btn /T (Agree) /V /Off "
    b"/Rect [20 30 32 42] /P 3 0 R /AS /Off /AP << /N %s >> >>" % inline(b"Yes")
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

out = "fixtures/synthetic/forms/radio-regroup-form.pdf"
open(out, "wb").write(buf)
print("wrote", out, len(buf), "bytes")
