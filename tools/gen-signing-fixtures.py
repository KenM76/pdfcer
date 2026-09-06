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
    # `Pass 10.14` additions (built from the EXISTING rsa2048 material unless
    # `--regen`, so adding a shape does not re-mint every key):
    "rsa2048-nolocalkeyid.pfx",
    "ecp384-modern.pfx",
    "ecp384.cer",
    "ecp384.key.der",
]

LOCAL_KEY_ID = "local_key_id"  # asn1crypto's name for 1.2.840.113549.1.9.21


def pkcs12_kdf(password: str, salt: bytes, iterations: int, key_id: int, n: int) -> bytes:
    """RFC 7292 Appendix B.2 with SHA-256 (u = 32, v = 64) — the same
    derivation `pdfcer-core::sign::pkcs12` performs, ported so the MAC of a
    hand-edited container can be recomputed. Password enters as BMPString
    with a trailing NUL (P12-11)."""
    import hashlib
    u, v = 32, 64
    d = bytes([key_id]) * v
    pw = password.encode("utf-16-be") + b"\x00\x00"

    def repeat_to_v(src: bytes) -> bytes:
        if not src:
            return b""
        length = v * -(-len(src) // v)
        return (src * (length // len(src) + 1))[:length]

    i = bytearray(repeat_to_v(salt) + repeat_to_v(pw))
    out = b""
    for _ in range(-(-n // u)):
        a = hashlib.sha256(d + bytes(i)).digest()
        for _ in range(1, iterations):
            a = hashlib.sha256(a).digest()
        out += a
        b = (a * (v // len(a) + 1))[:v]
        b_int = int.from_bytes(b, "big") + 1
        for off in range(0, len(i), v):
            block = int.from_bytes(i[off:off + v], "big") + b_int
            i[off:off + v] = (block & ((1 << (8 * v)) - 1)).to_bytes(v, "big")
    return out[:n]


def strip_local_key_id(src: Path, dst: Path) -> None:
    """Remove every `localKeyId` bag attribute from a PFX whose bags sit in
    PLAINTEXT `data` SafeContents (export with `-certpbe NONE` so the cert
    bags are not inside `encryptedData`), then recompute the SHA-256 MAC
    over the rewritten authSafe. The result pairs key and leaf ONLY by
    public-key identity — the fallback `Pkcs12Signer::from_der` documents."""
    import hmac
    import hashlib
    from asn1crypto import core, pkcs12 as p12

    pfx = p12.Pfx.load(src.read_bytes())
    auth = p12.AuthenticatedSafe.load(pfx["auth_safe"]["content"].native)
    removed = 0
    new_cis = []
    for ci in auth:
        if ci["content_type"].native != "data":
            raise SystemExit("strip_local_key_id: a SafeContents is encrypted; export with -certpbe NONE")
        sc = p12.SafeContents.load(ci["content"].native)
        bags = []
        for bag in sc:
            kept = [a for a in bag["bag_attributes"] if a["type"].native != LOCAL_KEY_ID]
            removed += len(bag["bag_attributes"]) - len(kept)
            bags.append(p12.SafeBag({"bag_id": bag["bag_id"], "bag_value": bag["bag_value"], "bag_attributes": p12.Attributes(kept)}))
        new_cis.append(p12.ContentInfo({"content_type": "data", "content": core.OctetString(p12.SafeContents(bags).dump(force=True))}))
    auth_bytes = p12.AuthenticatedSafe(new_cis).dump(force=True)
    mac_data = pfx["mac_data"]
    assert mac_data["mac"]["digest_algorithm"]["algorithm"].native == "sha256"
    salt = mac_data["mac_salt"].native
    iterations = mac_data["iterations"].native
    key = pkcs12_kdf(PASSWORD, salt, iterations, 3, 32)
    mac = hmac.new(key, auth_bytes, hashlib.sha256).digest()
    out = p12.Pfx({
        "version": pfx["version"],
        "auth_safe": p12.ContentInfo({"content_type": "data", "content": core.OctetString(auth_bytes)}),
        "mac_data": p12.MacData({"mac": {"digest_algorithm": {"algorithm": "sha256"}, "digest": mac}, "mac_salt": salt, "iterations": iterations}),
    })
    dst.write_bytes(out.dump(force=True))
    if removed < 2:
        raise SystemExit(f"strip_local_key_id: expected to remove a localKeyId from the key AND the cert bag, removed {removed}")


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


def added_shapes(work: Path, rsa_key: Path, rsa_cert: Path, major: int) -> None:
    """`Pass 10.14`: the stripped-`localKeyId` RSA store and the EC P-384 store."""
    plain = work / "rsa-plaincerts.pfx"
    run("openssl", "pkcs12", "-export", "-inkey", str(rsa_key), "-in", str(rsa_cert),
        "-name", "pdfcer-rsa-nolkid", "-passout", f"pass:{PASSWORD}",
        "-certpbe", "NONE", "-keypbe", "AES-256-CBC", "-macalg", "sha256", "-out", str(plain))
    strip_local_key_id(plain, OUT / "rsa2048-nolocalkeyid.pfx")

    ec_key = work / "ec384.key.pem"
    ec_cert = work / "ec384.cert.pem"
    run("openssl", "ecparam", "-name", "secp384r1", "-genkey", "-noout", "-out", str(ec_key))
    run("openssl", "req", "-x509", "-new", "-key", str(ec_key), "-out", str(ec_cert),
        "-days", DAYS, "-sha384",
        "-subj", "/CN=pdfcer synthetic EC P-384 signer (test fixture, trust nothing)/O=pdfcer fixtures/C=CA")
    export_pfx(work, ec_key, ec_cert, "pdfcer-ec384", OUT / "ecp384-modern.pfx", True, major)
    run("openssl", "x509", "-in", str(ec_cert), "-outform", "DER", "-out", str(OUT / "ecp384.cer"))
    run("openssl", "pkcs8", "-topk8", "-nocrypt", "-in", str(ec_key), "-outform", "DER",
        "-out", str(OUT / "ecp384.key.der"))
    for f in ("rsa2048-nolocalkeyid.pfx", "ecp384-modern.pfx", "ecp384.cer", "ecp384.key.der"):
        print(f"wrote {OUT / f} ({(OUT / f).stat().st_size} B)")


def main() -> int:
    if "--check" in sys.argv:
        missing = [f for f in EXPECTED if not (OUT / f).exists()]
        for f in missing:
            print(f"missing: {OUT / f}")
        return 1 if missing else 0

    major = openssl_major()
    OUT.mkdir(parents=True, exist_ok=True)
    regen = "--regen" in sys.argv or not (OUT / "rsa2048.key.der").exists()
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)
        if not regen:
            print("base stores exist; adding the Pass 10.14 shapes only (pass --regen to re-mint everything)")
            rsa_key = work / "rsa.key.pem"
            rsa_cert = work / "rsa.cert.pem"
            run("openssl", "pkey", "-inform", "DER", "-in", str(OUT / "rsa2048.key.der"), "-out", str(rsa_key))
            run("openssl", "x509", "-inform", "DER", "-in", str(OUT / "rsa2048.cer"), "-out", str(rsa_cert))
            added_shapes(work, rsa_key, rsa_cert, major)
            return 0

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

        added_shapes(work, rsa_key, rsa_cert, major)

    for f in EXPECTED:
        print(f"wrote {OUT / f} ({(OUT / f).stat().st_size} B)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
