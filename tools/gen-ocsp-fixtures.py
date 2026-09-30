#!/usr/bin/env python3
"""Generate the synthetic OCSP revocation fixtures in `fixtures/synthetic/ocsp/`.

Falsifiers for pdfcer's RFC 6960 OCSP response reader and revocation check.
pdfcer reads OCSP responses; pyca/cryptography (Apache-2.0/BSD, generator-side
only, never linked) writes them, so the reader is checked against an
implementation it did not write.

Keys are random per run: re-running REPLACES the corpus. The committed files
are the fixture. Every identity names itself a test fixture.

    ca.cer                  EC P-256 root CA (cA, keyCertSign + cRLSign)
    leaf.cer, leaf.pfx      EC P-256 end entity issued by ca, serial 0x1017;
                            the pfx holds the leaf key, leaf and ca
                            (password `pdfcer`)
    responder.cer           delegated responder issued by ca: EKU
                            id-kp-OCSPSigning, id-pkix-ocsp-nocheck
    responder-noeku.cer     issued by ca, no EKU (not authorized)
    responder-checked.cer   issued by ca, id-kp-OCSPSigning, NO nocheck:
                            usable only when a CRL covers it
    ocsp-good.der           signed by ca, ResponderID byName, good
    ocsp-good-delegated.der signed by responder (carried in certs), byKey
    ocsp-good-sha256.der    as good, CertID hashed with SHA-256
    ocsp-revoked.der        revoked 2026-09-15, keyCompromise (1)
    ocsp-revoked-after.der  revoked 2026-10-15, superseded (4)
    ocsp-unknown.der        status unknown
    ocsp-stale.der          good, nextUpdate 2026-09-10 (past)
    ocsp-noeku.der          good, signed by responder-noeku
    ocsp-forged.der         good, byName ca, signed by a different key
    ocsp-other-serial.der   good, for a different certificate of ca only
    ocsp-trylater.der       responseStatus tryLater (3), no responseBytes
    ocsp-basic-only.der     ocsp-good's bare BasicOCSPResponse (legacy DSS)
    ocsp-good-checked.der   good, signed by responder-checked (in certs)
    crl-clear.crl           ca's CRL listing only an unrelated serial
    crl-responder-revoked.crl  ca's CRL listing responder-checked

Pdfcer's tests use a reference time of 2026-09-30, between the two
revocation dates.
"""

from __future__ import annotations

import datetime as dt
import hashlib
import sys
from pathlib import Path

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.hazmat.primitives.serialization import pkcs12
from cryptography.x509 import ocsp
from cryptography.x509.oid import ExtendedKeyUsageOID, NameOID

OUT = Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "ocsp"
UTC = dt.timezone.utc
NOT_BEFORE = dt.datetime(2026, 1, 1, tzinfo=UTC)
NOT_AFTER = dt.datetime(2036, 1, 1, tzinfo=UTC)
THIS_UPDATE = dt.datetime(2026, 9, 20, tzinfo=UTC)
NEXT_UPDATE = dt.datetime(2099, 1, 1, tzinfo=UTC)


def name(cn: str) -> x509.Name:
    return x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, cn)])


CA_NAME = name("pdfcer OCSP test CA (test fixture, trust nothing)")


def issued(key, subject, serial, ca_key, extensions=()) -> x509.Certificate:
    b = (
        x509.CertificateBuilder()
        .subject_name(subject)
        .issuer_name(CA_NAME)
        .public_key(key.public_key())
        .serial_number(serial)
        .not_valid_before(NOT_BEFORE)
        .not_valid_after(NOT_AFTER)
        .add_extension(x509.BasicConstraints(ca=False, path_length=None), True)
    )
    for ext, critical in extensions:
        b = b.add_extension(ext, critical)
    return b.sign(ca_key, hashes.SHA256())


def response(
    cert,
    issuer,
    signer_cert,
    signer_key,
    status=ocsp.OCSPCertStatus.GOOD,
    revoked=None,
    reason=None,
    nxt=NEXT_UPDATE,
    by_key=False,
    certs=(),
    certid_hash=None,
) -> bytes:
    b = (
        ocsp.OCSPResponseBuilder()
        .add_response(
            cert=cert,
            issuer=issuer,
            algorithm=certid_hash or hashes.SHA1(),
            cert_status=status,
            this_update=THIS_UPDATE,
            next_update=nxt,
            revocation_time=revoked,
            revocation_reason=reason,
        )
        .responder_id(
            ocsp.OCSPResponderEncoding.HASH if by_key else ocsp.OCSPResponderEncoding.NAME,
            signer_cert,
        )
    )
    if certs:
        b = b.certificates(list(certs))
    return b.sign(signer_key, hashes.SHA256()).public_bytes(serialization.Encoding.DER)


def crl(ca_key, serial: int) -> bytes:
    entry = (
        x509.RevokedCertificateBuilder()
        .serial_number(serial)
        .revocation_date(dt.datetime(2026, 9, 1, tzinfo=UTC))
        .build()
    )
    return (
        x509.CertificateRevocationListBuilder()
        .issuer_name(CA_NAME)
        .last_update(THIS_UPDATE)
        .next_update(NEXT_UPDATE)
        .add_extension(x509.CRLNumber(1), False)
        .add_revoked_certificate(entry)
        .sign(ca_key, hashes.SHA256())
        .public_bytes(serialization.Encoding.DER)
    )


def read_tlv(buf: bytes, at: int) -> tuple[int, int, int]:
    """(tag, content start, content end) of the TLV at `at`."""
    tag = buf[at]
    first = buf[at + 1]
    if first < 0x80:
        return tag, at + 2, at + 2 + first
    n = first & 0x7F
    length = int.from_bytes(buf[at + 2 : at + 2 + n], "big")
    start = at + 2 + n
    return tag, start, start + length


def basic_only(full: bytes) -> bytes:
    """The BasicOCSPResponse inside an OCSPResponse's responseBytes."""
    _, s, _ = read_tlv(full, 0)  # OCSPResponse SEQUENCE
    _, _, e = read_tlv(full, s)  # responseStatus
    _, s, _ = read_tlv(full, e)  # [0] EXPLICIT
    _, s, _ = read_tlv(full, s)  # ResponseBytes SEQUENCE
    _, _, e = read_tlv(full, s)  # responseType OID
    tag, s, e = read_tlv(full, e)  # response OCTET STRING
    assert tag == 0x04
    return full[s:e]


def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    ca_key = ec.generate_private_key(ec.SECP256R1())
    ca = (
        x509.CertificateBuilder()
        .subject_name(CA_NAME)
        .issuer_name(CA_NAME)
        .public_key(ca_key.public_key())
        .serial_number(0x1000)
        .not_valid_before(NOT_BEFORE)
        .not_valid_after(NOT_AFTER)
        .add_extension(x509.BasicConstraints(ca=True, path_length=None), True)
        .add_extension(
            x509.KeyUsage(
                digital_signature=True,
                content_commitment=False,
                key_encipherment=False,
                data_encipherment=False,
                key_agreement=False,
                key_cert_sign=True,
                crl_sign=True,
                encipher_only=False,
                decipher_only=False,
            ),
            True,
        )
        .sign(ca_key, hashes.SHA256())
    )
    leaf_key = ec.generate_private_key(ec.SECP256R1())
    leaf = issued(leaf_key, name("pdfcer OCSP test signer (test fixture, trust nothing)"), 0x1017, ca_key)
    other_key = ec.generate_private_key(ec.SECP256R1())
    other = issued(other_key, name("pdfcer OCSP other subject (test fixture, trust nothing)"), 0x2017, ca_key)
    resp_key = ec.generate_private_key(ec.SECP256R1())
    responder = issued(
        resp_key,
        name("pdfcer OCSP test responder (test fixture, trust nothing)"),
        0x3017,
        ca_key,
        [
            (x509.ExtendedKeyUsage([ExtendedKeyUsageOID.OCSP_SIGNING]), False),
            (x509.OCSPNoCheck(), False),
        ],
    )
    noeku_key = ec.generate_private_key(ec.SECP256R1())
    noeku = issued(noeku_key, name("pdfcer OCSP unauthorized responder (test fixture, trust nothing)"), 0x4017, ca_key)
    checked_key = ec.generate_private_key(ec.SECP256R1())
    checked = issued(
        checked_key,
        name("pdfcer OCSP checked responder (test fixture, trust nothing)"),
        0x5017,
        ca_key,
        [(x509.ExtendedKeyUsage([ExtendedKeyUsageOID.OCSP_SIGNING]), False)],
    )
    forger = ec.generate_private_key(ec.SECP256R1())
    # Same subject as ca, forger's key: the response names ca but ca did not sign it.
    fake_ca = (
        x509.CertificateBuilder()
        .subject_name(CA_NAME)
        .issuer_name(CA_NAME)
        .public_key(forger.public_key())
        .serial_number(0x1000)
        .not_valid_before(NOT_BEFORE)
        .not_valid_after(NOT_AFTER)
        .sign(forger, hashes.SHA256())
    )
    r = x509.ReasonFlags
    S = ocsp.OCSPCertStatus
    good = response(leaf, ca, ca, ca_key)

    files = {
        "ca.cer": ca.public_bytes(serialization.Encoding.DER),
        "leaf.cer": leaf.public_bytes(serialization.Encoding.DER),
        "leaf.pfx": pkcs12.serialize_key_and_certificates(
            b"pdfcer-ocsp-leaf",
            leaf_key,
            leaf,
            [ca],
            serialization.BestAvailableEncryption(b"pdfcer"),
        ),
        "responder.cer": responder.public_bytes(serialization.Encoding.DER),
        "responder-noeku.cer": noeku.public_bytes(serialization.Encoding.DER),
        "ocsp-good.der": good,
        "ocsp-good-delegated.der": response(
            leaf, ca, responder, resp_key, by_key=True, certs=[responder]
        ),
        "ocsp-good-sha256.der": response(leaf, ca, ca, ca_key, certid_hash=hashes.SHA256()),
        "ocsp-revoked.der": response(
            leaf, ca, ca, ca_key, S.REVOKED, dt.datetime(2026, 9, 15, tzinfo=UTC), r.key_compromise
        ),
        "ocsp-revoked-after.der": response(
            leaf, ca, ca, ca_key, S.REVOKED, dt.datetime(2026, 10, 15, tzinfo=UTC), r.superseded
        ),
        "ocsp-unknown.der": response(leaf, ca, ca, ca_key, S.UNKNOWN),
        "ocsp-stale.der": response(leaf, ca, ca, ca_key, nxt=dt.datetime(2026, 9, 10, tzinfo=UTC)),
        "ocsp-noeku.der": response(leaf, ca, noeku, noeku_key, certs=[noeku]),
        "ocsp-forged.der": response(leaf, ca, fake_ca, forger),
        "ocsp-other-serial.der": response(other, ca, ca, ca_key),
        "ocsp-trylater.der": ocsp.OCSPResponseBuilder.build_unsuccessful(
            ocsp.OCSPResponseStatus.TRY_LATER
        ).public_bytes(serialization.Encoding.DER),
        "ocsp-basic-only.der": basic_only(good),
        "responder-checked.cer": checked.public_bytes(serialization.Encoding.DER),
        "ocsp-good-checked.der": response(leaf, ca, checked, checked_key, certs=[checked]),
        "crl-clear.crl": crl(ca_key, 0x999),
        "crl-responder-revoked.crl": crl(ca_key, 0x5017),
    }
    for fname, data in files.items():
        (OUT / fname).write_bytes(data)
        print(f"{fname:24} {len(data):5} {hashlib.sha256(data).hexdigest()[:16]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
