#!/usr/bin/env python3
"""gen-signing-fixtures.py — synthetic PKCS#12 key stores for the SIGNING tests.

WHAT THIS PRODUCES (under `fixtures/synthetic/signing/`)
=======================================================

Category (a) fixtures under `docs/LEGAL.md` §5: wholly synthetic key material
minted by this script with OpenSSL, for a certificate subject that names
itself as a test artefact. No real person, organisation or CA is involved;
nothing here is trusted by anything, and nothing here must ever be used to
sign a document anyone relies on.

| File | Key | Container encryption (RFC 7292 Appendix B era) | Why it exists |
|---|---|---|---|
| `rsa2048-modern.pfx` | RSA-2048 | **PBES2** / PBKDF2 / AES-256-CBC, MAC SHA-256 | the shape OpenSSL 3.x, recent Windows and Java export (`P12-9` "modern") |
| `rsa2048-legacy.pfx` | same RSA-2048 key + cert | **PKCS#12 PBE**: key `pbeWithSHAAnd3-KeyTripleDES-CBC`, certs `pbeWithSHAAnd40BitRC2-CBC`, MAC SHA-1 | the installed-base shape (`P12-9`/`P12-10` "legacy", OpenSSL 1.x defaults) — an importer must read BOTH |
| `ecp256-modern.pfx` | EC P-256 | PBES2 / AES-256-CBC | the ECDSA signing path |
| `rsa2048.cer`, `ecp256.cer` | — | DER X.509, unencrypted | so a test can assert the chain pdfcer extracted equals the certificate on disk byte-for-byte |
| `rsa2048.key.der`, `ecp256.key.der` | PKCS#8 PrivateKeyInfo, unencrypted | — | for the OpenSSL ORACLE only (`openssl cms -sign` / `-verify` against pdfcer's output). Tests never load these through pdfcer. |

Every container's password is `pdfcer` (ASCII, so `P12-11`'s BMPString
question has one answer for the MAC and the bags). The legacy and modern RSA
containers wrap the SAME key and certificate, so a test can prove the two
decryption eras yield identical material — which is the point of carrying two.

WHY OPENSSL, AND WHY THIS EXACT VERSION MATTERS
==============================================

pdfcer has no PKCS#12 *writer* (only import is in scope, `security__pkcs12_import.md`
§0), so the fixtures must come from an independent producer — which is also what
makes them an oracle rather than a mirror of pdfcer's own reading. OpenSSL
1.1.1 is on this machine (`openssl version` → 1.1.1s). Its `pkcs12 -export`
DEFAULTS are the legacy PKCS#12 PBE schemes, so the "legacy" file needs no
flags and the "modern" one needs `-keypbe/-certpbe AES-256-CBC -macalg sha256`.
Under OpenSSL 3.x the defaults flip (PBES2/AES-256, SHA-256 MAC) and the legacy
file would need `-legacy`; the script detects the major version and passes the
right flags either way, so regeneration on a different box still yields the
two eras the table promises.

DETERMINISM
===========

Key generation is random, so regenerating REPLACES the key material and every
committed byte changes. That is acceptable for these fixtures because no test
asserts a specific signature value — they assert round trips (pdfcer signs →
pdfcer AND OpenSSL verify), extracted-chain equality against the `.cer` beside
the store, and refusals. Validity is `-days 36500` (~100 years) so a test does
not start failing on a calendar date. Regenerate only when a fixture must gain a
new shape, and say so in `PROVENANCE.md`.

USAGE
=====

    python tools/gen-signing-fixtures.py            # writes fixtures/synthetic/signing/
    python tools/gen-signing-fixtures.py --check    # exit 1 if any expected file is missing

Exit codes: 0 success; 1 `--check` found a missing file; 2 OpenSSL missing or a
command failed (its stderr is printed).
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "fixtures" / "synthetic" / "signing"
PASSWORD = "pdfcer"
DAYS = "36500"

EXPECTED = [
    "rsa2048-modern.pfx",
    "rsa2048-legacy.pfx",
    "ecp256-modern.pfx",
    "rsa2048.cer",
    "ecp256.cer",
    "rsa2048.key.der",
    "ecp256.key.der",
]


def run(*args: str) -> None:
    """Run one OpenSSL command; on failure print its stderr and exit 2.

    A fixture generator that half-succeeds leaves a directory that LOOKS
    complete, so every command is fatal.
    """
    proc = subprocess.run(list(args), capture_output=True, text=True)
    if proc.returncode != 0:
        sys.stderr.write(f"gen-signing-fixtures: command failed: {' '.join(args)}\n")
        sys.stderr.write(proc.stderr)
        sys.exit(2)


def openssl_major() -> int:
    proc = subprocess.run(["openssl", "version"], capture_output=True, text=True)
    if proc.returncode != 0:
        sys.stderr.write("gen-signing-fixtures: openssl is not on PATH\n")
        sys.exit(2)
    # "OpenSSL 1.1.1s  1 Nov 2022" / "OpenSSL 3.2.1 30 Jan 2024"
    return int(proc.stdout.split()[1].split(".")[0])


def export_pfx(work: Path, key_pem: Path, cert_pem: Path, name: str, out: Path, modern: bool, major: int) -> None:
    """`pkcs12 -export` with the flags that pin the encryption ERA regardless
    of which OpenSSL is doing the exporting (see the module docs)."""
    args = [
        "openssl", "pkcs12", "-export",
        "-inkey", str(key_pem), "-in", str(cert_pem),
        "-name", name,
        "-passout", f"pass:{PASSWORD}",
        "-out", str(out),
    ]
    if modern:
        args += ["-keypbe", "AES-256-CBC", "-certpbe", "AES-256-CBC", "-macalg", "sha256"]
    else:
        # RFC 7292 Appendix B legacy schemes, spelled explicitly so the file
        # is legacy on OpenSSL 3.x too (where the defaults are PBES2).
        args += [
            "-keypbe", "PBE-SHA1-3DES",
            "-certpbe", "PBE-SHA1-RC2-40",
            "-macalg", "sha1",
        ]
        if major >= 3:
            args += ["-legacy"]
    run(*args)


def main() -> int:
    if "--check" in sys.argv:
        missing = [f for f in EXPECTED if not (OUT / f).exists()]
        for f in missing:
            print(f"missing: {OUT / f}")
        return 1 if missing else 0

    major = openssl_major()
    OUT.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)

        # --- RSA-2048 -------------------------------------------------------
        rsa_key = work / "rsa.key.pem"
        rsa_cert = work / "rsa.cert.pem"
        run("openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes",
            "-keyout", str(rsa_key), "-out", str(rsa_cert), "-days", DAYS,
            "-sha256",
            "-subj", "/CN=pdfcer synthetic RSA signer (test fixture, trust nothing)/O=pdfcer fixtures/C=CA")
        export_pfx(work, rsa_key, rsa_cert, "pdfcer-rsa", OUT / "rsa2048-modern.pfx", True, major)
        export_pfx(work, rsa_key, rsa_cert, "pdfcer-rsa", OUT / "rsa2048-legacy.pfx", False, major)
        run("openssl", "x509", "-in", str(rsa_cert), "-outform", "DER", "-out", str(OUT / "rsa2048.cer"))
        run("openssl", "pkcs8", "-topk8", "-nocrypt", "-in", str(rsa_key), "-outform", "DER",
            "-out", str(OUT / "rsa2048.key.der"))

        # --- EC P-256 -------------------------------------------------------
        ec_key = work / "ec.key.pem"
        ec_cert = work / "ec.cert.pem"
        run("openssl", "ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", str(ec_key))
        run("openssl", "req", "-x509", "-new", "-key", str(ec_key), "-out", str(ec_cert),
            "-days", DAYS, "-sha256",
            "-subj", "/CN=pdfcer synthetic EC P-256 signer (test fixture, trust nothing)/O=pdfcer fixtures/C=CA")
        export_pfx(work, ec_key, ec_cert, "pdfcer-ec", OUT / "ecp256-modern.pfx", True, major)
        run("openssl", "x509", "-in", str(ec_cert), "-outform", "DER", "-out", str(OUT / "ecp256.cer"))
        run("openssl", "pkcs8", "-topk8", "-nocrypt", "-in", str(ec_key), "-outform", "DER",
            "-out", str(OUT / "ecp256.key.der"))

    for f in EXPECTED:
        print(f"wrote {OUT / f} ({(OUT / f).stat().st_size} B)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
