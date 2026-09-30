#!/usr/bin/env python3
"""Generate the synthetic CRL revocation fixtures in `fixtures/synthetic/crl/`.

Falsifiers for pdfcer's RFC 5280 §5 CRL reader and revocation check. pdfcer
reads CRLs; pyca/cryptography (Apache-2.0/BSD, generator-side only, never
linked) writes them, so the reader is checked against an implementation
it did not write.

Keys are random per run: re-running REPLACES the corpus. The committed files
are the fixture. Every identity names itself a test fixture.

    ca.cer                  EC P-256 CA; basicConstraints cA, keyUsage
                            keyCertSign + cRLSign
    ca-nocrlsign.cer        the SAME key and subject, keyUsage keyCertSign
                            only (RFC 10007 §6.3.3(f): must not sign CRLs)
    leaf.cer, leaf.pfx      EC P-256 end entity issued by ca; the pfx holds
                            the leaf key, the leaf and ca (password `pdfcer`)
    crl-good.crl            ca's CRL listing only an unrelated serial
    crl-revoked.crl         lists leaf, 2026-09-15, keyCompromise (1)
    crl-revoked-after.crl   lists leaf, 2026-10-15, superseded (4)
    crl-stale.crl           as good, but nextUpdate 2026-09-10 (past)
    crl-critical.crl        as good, plus an unknown CRITICAL extension
    crl-delta.crl           as good, plus deltaCRLIndicator (critical)
    crl-forged.crl          ca's name, a DIFFERENT key, lists nothing
    crl-idp-user.crl        as good, issuingDistributionPoint onlyContainsUserCerts
    crl-idp-other.crl       as good, issuingDistributionPoint naming a partition
                            URI the leaf does not name (the leaf names none)
    crl-other-issuer.crl    a different CA name and key, lists leaf

Pdfcer's tests sign with `--signing-time D:20260930000000Z`, between the
two revocation dates.
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
from cryptography.x509.oid import NameOID

OUT = Path(__file__).resolve().parent.parent / "fixtures" / "synthetic" / "crl"
UTC = dt.timezone.utc
NOT_BEFORE = dt.datetime(2026, 1, 1, tzinfo=UTC)
NOT_AFTER = dt.datetime(2036, 1, 1, tzinfo=UTC)
LEAF_SERIAL = 0x1016
UNRELATED_SERIAL = 0x999


def name(cn: str) -> x509.Name:
    return x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, cn)])


CA_NAME = name("pdfcer CRL test CA (test fixture, trust nothing)")
OTHER_NAME = name("pdfcer other CA (test fixture, trust nothing)")


def key_usage(crl_sign: bool) -> x509.KeyUsage:
    return x509.KeyUsage(
        digital_signature=False,
        content_commitment=False,
        key_encipherment=False,
        data_encipherment=False,
        key_agreement=False,
        key_cert_sign=True,
        crl_sign=crl_sign,
        encipher_only=False,
        decipher_only=False,
    )


def ca_cert(key, subject, crl_sign: bool, serial: int) -> x509.Certificate:
    return (
        x509.CertificateBuilder()
        .subject_name(subject)
        .issuer_name(subject)
        .public_key(key.public_key())
        .serial_number(serial)
        .not_valid_before(NOT_BEFORE)
        .not_valid_after(NOT_AFTER)
        .add_extension(x509.BasicConstraints(ca=True, path_length=None), True)
        .add_extension(key_usage(crl_sign), True)
        .add_extension(x509.SubjectKeyIdentifier.from_public_key(key.public_key()), False)
        .sign(key, hashes.SHA256())
    )


def crl(issuer_name, key, revoked=(), this=None, nxt=None, extra=()) -> bytes:
    b = (
        x509.CertificateRevocationListBuilder()
        .issuer_name(issuer_name)
        .last_update(this or dt.datetime(2026, 9, 20, tzinfo=UTC))
        .next_update(nxt or dt.datetime(2099, 1, 1, tzinfo=UTC))
        .add_extension(x509.CRLNumber(1), False)
    )
    for serial, when, reason in revoked:
        entry = x509.RevokedCertificateBuilder().serial_number(serial).revocation_date(when)
        if reason is not None:
            entry = entry.add_extension(x509.CRLReason(reason), False)
        b = b.add_revoked_certificate(entry.build())
    for ext, critical in extra:
        b = b.add_extension(ext, critical)
    return b.sign(key, hashes.SHA256()).public_bytes(serialization.Encoding.DER)


def idp(uri, users=False) -> x509.IssuingDistributionPoint:
    return x509.IssuingDistributionPoint(
        full_name=[x509.UniformResourceIdentifier(uri)] if uri else None,
        relative_name=None,
        only_contains_user_certs=users,
        only_contains_ca_certs=False,
        only_some_reasons=None,
        indirect_crl=False,
        only_contains_attribute_certs=False,
    )


def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    ca_key = ec.generate_private_key(ec.SECP256R1())
    ca = ca_cert(ca_key, CA_NAME, True, 0x1000)
    ca_nocrl = ca_cert(ca_key, CA_NAME, False, 0x1001)
    leaf_key = ec.generate_private_key(ec.SECP256R1())
    leaf = (
        x509.CertificateBuilder()
        .subject_name(name("pdfcer CRL test signer (test fixture, trust nothing)"))
        .issuer_name(CA_NAME)
        .public_key(leaf_key.public_key())
        .serial_number(LEAF_SERIAL)
        .not_valid_before(NOT_BEFORE)
        .not_valid_after(NOT_AFTER)
        .add_extension(x509.BasicConstraints(ca=False, path_length=None), True)
        .add_extension(
            x509.AuthorityKeyIdentifier.from_issuer_public_key(ca_key.public_key()), False
        )
        .sign(ca_key, hashes.SHA256())
    )
    forger = ec.generate_private_key(ec.SECP256R1())
    other = ec.generate_private_key(ec.SECP256R1())
    r = x509.ReasonFlags
    before = dt.datetime(2026, 9, 15, tzinfo=UTC)
    after = dt.datetime(2026, 10, 15, tzinfo=UTC)
    unrelated = [(UNRELATED_SERIAL, before, None)]

    files = {
        "ca.cer": ca.public_bytes(serialization.Encoding.DER),
        "ca-nocrlsign.cer": ca_nocrl.public_bytes(serialization.Encoding.DER),
        "leaf.cer": leaf.public_bytes(serialization.Encoding.DER),
        "leaf.pfx": pkcs12.serialize_key_and_certificates(
            b"pdfcer-crl-leaf",
            leaf_key,
            leaf,
            [ca],
            serialization.BestAvailableEncryption(b"pdfcer"),
        ),
        "crl-good.crl": crl(CA_NAME, ca_key, unrelated),
        "crl-revoked.crl": crl(CA_NAME, ca_key, [(LEAF_SERIAL, before, r.key_compromise)]),
        "crl-revoked-after.crl": crl(
            CA_NAME,
            ca_key,
            [(LEAF_SERIAL, after, r.superseded)],
            this=dt.datetime(2026, 10, 20, tzinfo=UTC),
        ),
        "crl-stale.crl": crl(
            CA_NAME,
            ca_key,
            unrelated,
            this=dt.datetime(2026, 9, 1, tzinfo=UTC),
            nxt=dt.datetime(2026, 9, 10, tzinfo=UTC),
        ),
        "crl-critical.crl": crl(
            CA_NAME,
            ca_key,
            unrelated,
            extra=[(x509.UnrecognizedExtension(x509.ObjectIdentifier("1.3.6.1.4.1.55555.1"), b"\x05\x00"), True)],
        ),
        "crl-delta.crl": crl(CA_NAME, ca_key, unrelated, extra=[(x509.DeltaCRLIndicator(1), True)]),
        "crl-forged.crl": crl(CA_NAME, forger, unrelated),
        "crl-idp-user.crl": crl(CA_NAME, ca_key, unrelated, extra=[(idp(None, users=True), True)]),
        "crl-idp-other.crl": crl(
            CA_NAME,
            ca_key,
            unrelated,
            extra=[(idp("http://crl.example.invalid/part2.crl"), True)],
        ),
        "crl-other-issuer.crl": crl(OTHER_NAME, other, [(LEAF_SERIAL, before, r.key_compromise)]),
    }
    for fname, data in files.items():
        (OUT / fname).write_bytes(data)
        print(f"{fname:24} {len(data):5} {hashlib.sha256(data).hexdigest()[:16]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
