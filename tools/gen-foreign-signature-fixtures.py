#!/usr/bin/env python3
"""gen-foreign-signature-fixtures.py — PDFs signed by a FOREIGN signer (pyHanko).

Pass 10.14 criterion 3: pdfcer must add an approval signature ON TOP of a
signature it did not write (both verify, the foreign `/ByteRange` intact),
and — the other direction — a foreign tool must be able to countersign a
document pdfcer signed. Neither can be measured with pdfcer's own output
alone, so this generator uses **pyHanko** (MIT, a generator-side tool only —
never linked; rule 13) over pdfcer's synthetic `hello.pdf` and pdfcer's own
synthetic PKCS#12 stores (`gen-signing-fixtures.py`, password `pdfcer`).

Outputs (fixtures/synthetic/signing/):

  foreign-pyhanko-first.pdf    hello.pdf + ONE pyHanko signature
                               (field `ForeignSig`, PAdES CAdES-detached,
                               RSA-2048 from rsa2048-modern.pfx). pdfcer's
                               test signs on top of this.
  pdfcer-then-pyhanko.pdf      hello.pdf signed by `pdfcer sign` (field
                               `Signature1`), then countersigned by pyHanko
                               (field `ForeignSig`). pdfcer's test verifies
                               BOTH. Needs a built `pdfcer.exe`; pass its
                               path as `--pdfcer <exe>` (default:
                               target/release/pdfcer.exe). Skipped, with a
                               message, when the binary is absent — the
                               test then records this direction as NOT
                               MEASURED rather than passed.

Not reproducible byte-for-byte: pyHanko stamps the signing time from the
clock. The committed bytes are the fixture; re-running replaces them.
"""
from __future__ import annotations

import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "fixtures" / "synthetic" / "signing"
HELLO = ROOT / "fixtures" / "synthetic" / "hello.pdf"
PFX = OUT / "rsa2048-modern.pfx"
PASSWORD = b"pdfcer"


def pyhanko_sign(src: Path, dst: Path, field: str, reason: str) -> None:
    from pyhanko.pdf_utils.incremental_writer import IncrementalPdfFileWriter
    from pyhanko.sign import signers

    signer = signers.SimpleSigner.load_pkcs12(str(PFX), passphrase=PASSWORD)
    with src.open("rb") as f:
        w = IncrementalPdfFileWriter(f)
        meta = signers.PdfSignatureMetadata(field_name=field, reason=reason)
        out = signers.sign_pdf(w, meta, signer=signer)
    dst.write_bytes(out.getbuffer())
    print(f"wrote {dst} ({dst.stat().st_size} B) via pyHanko, field {field}")


def main() -> int:
    exe = ROOT / "target" / "release" / "pdfcer.exe"
    if "--pdfcer" in sys.argv:
        exe = Path(sys.argv[sys.argv.index("--pdfcer") + 1])
    if "--check" in sys.argv:
        missing = [f for f in ("foreign-pyhanko-first.pdf", "pdfcer-then-pyhanko.pdf") if not (OUT / f).exists()]
        for f in missing:
            print(f"missing: {OUT / f}")
        return 1 if missing else 0

    pyhanko_sign(HELLO, OUT / "foreign-pyhanko-first.pdf", "ForeignSig", "pyHanko first signature (fixture)")

    if not exe.exists():
        print(f"pdfcer binary not found at {exe}; pdfcer-then-pyhanko.pdf NOT generated")
        return 0
    mid = OUT / "_pdfcer-signed.tmp.pdf"
    subprocess.run(
        [str(exe), "sign", str(HELLO), "--cert", str(PFX), "--password", "pdfcer",
         "--signing-time", "D:20260906000000Z", "--output", str(mid)],
        check=True, capture_output=True,
    )
    pyhanko_sign(mid, OUT / "pdfcer-then-pyhanko.pdf", "ForeignSig", "pyHanko countersignature (fixture)")
    mid.unlink()
    return 0


if __name__ == "__main__":
    sys.exit(main())
