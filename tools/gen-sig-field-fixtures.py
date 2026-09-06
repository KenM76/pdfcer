#!/usr/bin/env python3
"""gen-sig-field-fixtures.py — pages carrying a PRE-PLACED, EMPTY signature field.

`Pass 10.13`: the "sign here" shape every form author produces — an
`/FT /Sig` field with a widget rectangle and NO `/V` — which `pdfcer sign
--field-name <name>` signs INTO rather than beside. Four variants, one page
each (612 x 792), a text field `Name` beside the signature field so the
"wrong field type" refusal has a target, and NO signature anywhere (nothing
here is signed; nothing here is a `.pfx`):

  sig-field-empty.pdf            /FT /Sig /T (SignHere) /Rect [72 600 300 660]
                                 /P <page>, merged widget, no /V, no /Lock,
                                 no /SV — the plain case.
  sig-field-lock.pdf             + /Lock << /Type /SigFieldLock /Action /All >>
                                 (Table 233) — signing must write a /FieldMDP
                                 reference copying Action/Fields (§12.8.2.4).
  sig-field-lock-include.pdf     + /Lock << /Action /Include /Fields [(Name)] >>
  sig-field-sv-ok.pdf            + /SV << /Type /SV /Ff 7 (bits 1|2|3 required:
                                 Filter, SubFilter, V) /Filter /Adobe.PPKLite
                                 /SubFilter [/ETSI.CAdES.detached
                                 /adbe.pkcs7.detached] /V 1.0 /DigestMethod
                                 [/SHA256 /SHA384] /Reasons [(Approved)
                                 (Reviewed)] /MDP << /P 0 >> >> (Table 234) —
                                 every constraint pdfcer can evaluate, all of
                                 them satisfiable by pdfcer's defaults.
  sig-field-sv-strict.pdf        + /SV << /Ff 8 (bit 4: Reasons required)
                                 /Reasons [(Only this reason)] /DigestMethod
                                 [/SHA1] /Ff 72 >> — evaluable and VIOLATED by
                                 a default request (wrong reason; SHA-1 is
                                 not a digest pdfcer will use), so the
                                 refusal-by-name branch has a target.
                                 (Ff 72 = bits 4 and 7: Reasons + DigestMethod
                                 required.)
  sig-field-sv-cert.pdf          + /SV << /Cert << /Ff 1 /Subject [(anyone)] >>
                                 >> (Table 235) — a constraint pdfcer does NOT
                                 evaluate; refused by name, never skipped.

Regenerate with `python tools/gen-sig-field-fixtures.py`; `--check` exits 1
if a file is missing. Deterministic: no clock, no randomness.
"""
from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "fixtures" / "synthetic" / "signing"

VARIANTS = {
    "sig-field-empty.pdf": "",
    "sig-field-lock.pdf": "/Lock << /Type /SigFieldLock /Action /All >>",
    "sig-field-lock-include.pdf": "/Lock << /Type /SigFieldLock /Action /Include /Fields [(Name)] >>",
    "sig-field-sv-ok.pdf": (
        "/SV << /Type /SV /Ff 7 /Filter /Adobe.PPKLite "
        "/SubFilter [/ETSI.CAdES.detached /adbe.pkcs7.detached] /V 1.0 "
        "/DigestMethod [/SHA256 /SHA384] /Reasons [(Approved) (Reviewed)] /MDP << /P 0 >> >>"
    ),
    "sig-field-sv-strict.pdf": (
        "/SV << /Type /SV /Ff 72 /Reasons [(Only this reason)] /DigestMethod [/SHA1] >>"
    ),
    "sig-field-sv-cert.pdf": "/SV << /Type /SV /Cert << /Ff 1 /Subject [(anyone)] >> >>",
}


def build(extra: str) -> bytes:
    content = (
        "BT /Helv 12 Tf 72 700 Td (Sign here:) Tj ET\n"
        "0 0 0 RG 0.5 w 72 600 228 60 re S\n"
    )
    objects = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [5 0 R 6 0 R] /SigFlags 0 "
        "/DA (/Helv 0 Tf 0 g) /DR << /Font << /Helv 7 0 R >> >> >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R "
        "/Resources << /Font << /Helv 7 0 R >> >> /Annots [5 0 R 6 0 R] >>",
        f"<< /Length {len(content)} >>\nstream\n{content}endstream",
        # 5: the pre-placed, EMPTY signature field (merged widget).
        "<< /Type /Annot /Subtype /Widget /FT /Sig /T (SignHere) /TU (Sign here) "
        f"/Rect [72 600 300 660] /P 3 0 R /F 4 {extra} >>",
        # 6: a text field, for the wrong-type refusal.
        "<< /Type /Annot /Subtype /Widget /FT /Tx /T (Name) /Rect [72 500 300 520] "
        "/P 3 0 R /F 4 /DA (/Helv 10 Tf 0 g) >>",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
    ]
    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets = []
    for n, body in enumerate(objects, start=1):
        offsets.append(len(out))
        out += f"{n} 0 obj\n{body}\nendobj\n".encode("latin-1")
    xref = len(out)
    out += f"xref\n0 {len(objects) + 1}\n0000000000 65535 f \n".encode()
    for off in offsets:
        out += f"{off:010} 00000 n \n".encode()
    out += (
        f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
    ).encode()
    return bytes(out)


def main() -> int:
    if "--check" in sys.argv:
        missing = [f for f in VARIANTS if not (OUT / f).exists()]
        for f in missing:
            print(f"missing: {OUT / f}")
        return 1 if missing else 0
    OUT.mkdir(parents=True, exist_ok=True)
    for name, extra in VARIANTS.items():
        data = build(extra)
        (OUT / name).write_bytes(data)
        print(f"wrote {OUT / name} ({len(data)} B)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
