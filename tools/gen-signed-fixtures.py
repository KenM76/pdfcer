#!/usr/bin/env python3
"""Generate CRYPTOGRAPHICALLY SIGNED PDFs (`fixtures/synthetic/signature-verify/`) as falsifiers for `pdfcer_core::signature::verify`.

Not to be confused with `gen-signature-fixtures.py`, which builds the
no-cryptography `/ByteRange` COVERAGE fixtures in `fixtures/synthetic/signature/`.
That one exercises offset arithmetic; this one exercises the digest and the
CMS signature check.

Source document is synthetic and self-authored (project rule 7): one page
of text written by this script. The SIGNATURES are produced by pyHanko
(MIT), an INDEPENDENT implementation of ISO 32000-1 §12.8.3 / ETSI EN 319
142-1 — the same principle as `gen-encryption-fixtures.py`: pdfcer's
verifier is written from the standard and then checked against CMS objects
it did not produce. The certificates are self-signed, generated fresh by
`cryptography` on every run, so nothing here is anyone's real identity and
the private keys are thrown away with the temp directory.

Variants (all `/Sig` fields named `Sig1`, all SHA-256 unless stated):

    sig-rsa-pkcs7-detached.pdf     adbe.pkcs7.detached, RSA-2048 PKCS#1 v1.5
    sig-rsa-pss-cades.pdf          ETSI.CAdES.detached, RSA-2048 RSASSA-PSS
    sig-ecdsa-p256-cades.pdf       ETSI.CAdES.detached, ECDSA P-256
    sig-rsa-sha1-pkcs7.pdf         adbe.pkcs7.detached, RSA-2048, SHA-1 digest
    sig-rsa-tampered.pdf           the first file with one byte of the page
                                   content flipped INSIDE the signed range —
                                   integrity must FAIL, coverage still to EOF
    sig-rsa-appended.pdf           the first file plus an incremental update
                                   (a /Info change) AFTER the signature —
                                   integrity must PASS, coverage must say
                                   "bytes after the signed range"
    sig-rsa-contents-tampered.pdf  the first file with one byte of the CMS
                                   signature value flipped — the signature
                                   check (not the digest) must fail

Every file's provenance is written to PROVENANCE.md beside it, with the
SHA-256 of each output so a regenerated corpus is recognisable as new.

★ The keys are random per run, so re-running REPLACES the corpus with a
different one. That is deliberate (no private key ever exists on disk for
longer than this process) and it means the committed files are the fixture,
not the script's output on a given day. Extend by adding a variant and
re-running; do not expect byte identity with the previous corpus.
"""

from __future__ import annotations

# ★ The certificate names and the /Reason string deliberately keep the
# PRE-RELEASE name `pdfce`: the checked-in fixtures under
# fixtures/synthetic/signature-verify/ were generated with it, and
# crates/pdfcer-core/tests/signature_verify.rs asserts on those bytes.
# Regenerating with `pdfcer` would silently change what the tests expect.
# (Pass 247.1, 2026-09-03.)

import datetime as dt
import hashlib
import io
import os
import sys
import tempfile

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec, rsa
from cryptography.x509.oid import NameOID
from pyhanko.sign import signers, fields
from pyhanko.sign.fields import SigSeedSubFilter
from pyhanko.pdf_utils.incremental_writer import IncrementalPdfFileWriter
from pyhanko.pdf_utils import generic

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "..", "fixtures", "synthetic", "signature-verify")

CONTENT = b"BT /F1 24 Tf 40 120 Td (SIGNED CONTENT) Tj ET"


def base_pdf() -> bytes:
    """A one-page classic PDF with an empty AcroForm so pyHanko can add a
    signature field without restructuring the catalog."""
    bodies = [
        b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [] >> >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200] "
        b"/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        b"<< /Length %d >>\nstream\n" % len(CONTENT) + CONTENT + b"\nendstream",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ]
    buf = bytearray(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n")
    offsets = []
    for i, body in enumerate(bodies):
        offsets.append(len(buf))
        buf += b"%d 0 obj\n" % (i + 1) + body + b"\nendobj\n"
    xref = len(buf)
    buf += b"xref\n0 %d\n0000000000 65535 f \n" % (len(bodies) + 1)
    for off in offsets:
        buf += b"%010d 00000 n \n" % off
    buf += (
        b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n"
        % (len(bodies) + 1, xref)
    )
    return bytes(buf)


def self_signed(key, name: str):
    subject = issuer = x509.Name(
        [
            x509.NameAttribute(NameOID.COUNTRY_NAME, "CA"),
            x509.NameAttribute(NameOID.ORGANIZATION_NAME, "pdfce test fixtures"),
            x509.NameAttribute(NameOID.COMMON_NAME, name),
        ]
    )
    now = dt.datetime(2026, 1, 1, tzinfo=dt.timezone.utc)
    return (
        x509.CertificateBuilder()
        .subject_name(subject)
        .issuer_name(issuer)
        .public_key(key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(now)
        .not_valid_after(now + dt.timedelta(days=3650))
        .add_extension(x509.BasicConstraints(ca=True, path_length=None), critical=True)
        .sign(key, hashes.SHA256())
    )


def signer_for(key, cert, tmp: str, tag: str, prefer_pss: bool = False):
    key_path = os.path.join(tmp, f"{tag}.key")
    cert_path = os.path.join(tmp, f"{tag}.crt")
    with open(key_path, "wb") as f:
        f.write(
            key.private_bytes(
                serialization.Encoding.PEM,
                serialization.PrivateFormat.PKCS8,
                serialization.NoEncryption(),
            )
        )
    with open(cert_path, "wb") as f:
        f.write(cert.public_bytes(serialization.Encoding.PEM))
    return signers.SimpleSigner.load(key_path, cert_path, prefer_pss=prefer_pss)


def sign(pdf: bytes, signer, subfilter, md_algorithm: str) -> bytes:
    w = IncrementalPdfFileWriter(io.BytesIO(pdf))
    fields.append_signature_field(w, fields.SigFieldSpec(sig_field_name="Sig1"))
    meta = signers.PdfSignatureMetadata(
        field_name="Sig1",
        subfilter=subfilter,
        md_algorithm=md_algorithm,
        reason="pdfce verification fixture",
        location="synthetic",
    )
    out = signers.sign_pdf(w, meta, signer=signer)
    return out.getvalue()


def flip_inside_content(pdf: bytes) -> bytes:
    i = pdf.index(b"SIGNED CONTENT")
    b = bytearray(pdf)
    b[i] = ord("X")  # 'S' -> 'X', one byte inside the signed range
    return bytes(b)


def flip_in_contents(pdf: bytes) -> bytes:
    """Flip one hex digit near the END of the DER inside /Contents.

    The SignerInfo's `signature` OCTET STRING is the last element of the
    CMS, so the last non-padding bytes of the hex string are the signature
    value itself. Flipping there fails the SIGNATURE check while the
    digest over the byte range still matches — the second failure mode,
    distinct from a tampered document. Flipping in the middle would land
    in the embedded certificate instead (measured: pyHanko then reported a
    signing-certificate mismatch, not a bad signature)."""
    i = pdf.index(b"/Contents <")
    start = i + len(b"/Contents <")
    end = pdf.index(b">", start)
    hex_str = pdf[start:end]
    last = len(hex_str.rstrip(b"0")) - 1  # last non-padding hex digit
    target = start + last - 20
    b = bytearray(pdf)
    b[target] = ord("0") if b[target] != ord("0") else ord("1")
    return bytes(b)


def append_after(pdf: bytes) -> bytes:
    w = IncrementalPdfFileWriter(io.BytesIO(pdf))
    info = generic.DictionaryObject({generic.NameObject("/Title"): generic.TextStringObject("appended after signing")})
    w.trailer["/Info"] = w.add_object(info)
    out = io.BytesIO()
    w.write(out)
    return out.getvalue()


def main() -> int:
    os.makedirs(OUT, exist_ok=True)
    tmp = tempfile.mkdtemp(prefix="pdfcer-sig-")
    rsa_key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    ec_key = ec.generate_private_key(ec.SECP256R1())
    rsa_cert = self_signed(rsa_key, "pdfce fixture RSA signer")
    ec_cert = self_signed(ec_key, "pdfce fixture ECDSA signer")
    rsa_signer = signer_for(rsa_key, rsa_cert, tmp, "rsa")
    rsa_pss_signer = signer_for(rsa_key, rsa_cert, tmp, "rsa-pss", prefer_pss=True)
    ec_signer = signer_for(ec_key, ec_cert, tmp, "ec")

    base = base_pdf()
    files: dict[str, bytes] = {}
    files["sig-rsa-pkcs7-detached.pdf"] = sign(
        base, rsa_signer, SigSeedSubFilter.ADOBE_PKCS7_DETACHED, "sha256"
    )
    files["sig-rsa-pss-cades.pdf"] = sign(
        base, rsa_pss_signer, SigSeedSubFilter.PADES, "sha256"
    )
    files["sig-ecdsa-p256-cades.pdf"] = sign(base, ec_signer, SigSeedSubFilter.PADES, "sha256")
    files["sig-rsa-sha1-pkcs7.pdf"] = sign(
        base, rsa_signer, SigSeedSubFilter.ADOBE_PKCS7_DETACHED, "sha1"
    )
    files["sig-rsa-tampered.pdf"] = flip_inside_content(files["sig-rsa-pkcs7-detached.pdf"])
    files["sig-rsa-contents-tampered.pdf"] = flip_in_contents(files["sig-rsa-pkcs7-detached.pdf"])
    files["sig-rsa-appended.pdf"] = append_after(files["sig-rsa-pkcs7-detached.pdf"])

    prov = [
        "# Signature fixtures — provenance\n",
        "Generated by `tools/gen-signed-fixtures.py` (read its docstring). Source",
        "document: synthetic, written by the script. Signatures: pyHanko "
        f"{__import__('importlib.metadata').metadata.version('pyhanko')} (MIT), an independent implementation.",
        "Certificates: self-signed, generated per run by `cryptography`, private",
        "keys discarded with the temp directory. No real identity, no real key.\n",
        "| file | what it is | sha256 |",
        "|---|---|---|",
    ]
    descr = {
        "sig-rsa-pkcs7-detached.pdf": "adbe.pkcs7.detached, RSA-2048 PKCS#1 v1.5, SHA-256 — VALID",
        "sig-rsa-pss-cades.pdf": "ETSI.CAdES.detached, RSA-2048 RSASSA-PSS, SHA-256 — VALID",
        "sig-ecdsa-p256-cades.pdf": "ETSI.CAdES.detached, ECDSA P-256, SHA-256 — VALID",
        "sig-rsa-sha1-pkcs7.pdf": "adbe.pkcs7.detached, RSA-2048, SHA-1 — VALID (SHA-1 is legal in 1.7)",
        "sig-rsa-tampered.pdf": "one byte of the page content flipped INSIDE the signed range — integrity FAILS (digest)",
        "sig-rsa-contents-tampered.pdf": "one hex digit of /Contents flipped — integrity FAILS (signature), digest still matches",
        "sig-rsa-appended.pdf": "an incremental update (/Info) AFTER signing — integrity PASSES, coverage stops before EOF",
    }
    for name, data in files.items():
        with open(os.path.join(OUT, name), "wb") as f:
            f.write(data)
        prov.append(f"| `{name}` | {descr[name]} | `{hashlib.sha256(data).hexdigest()}` |")
        print(f"wrote {name} ({len(data)} bytes)")
    with open(os.path.join(OUT, "PROVENANCE.md"), "w", encoding="utf-8") as f:
        f.write("\n".join(prov) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
