# fixtures/synthetic/ocsp — provenance

Synthetic. Minted by `tools/gen-ocsp-fixtures.py` with pyca/cryptography
(Apache-2.0/BSD; generator-side only, never linked). Every key is random per
run and discarded; every identity names itself a test fixture
("trust nothing"). Re-running replaces the corpus — the committed files are
the fixture. The generator's docstring lists each file and what it
exercises (RFC 6960 responses: good by name and by key, SHA-256 CertID,
bare BasicOCSPResponse, revoked before/after 2026-09-30, unknown, stale,
forged, a responder without id-kp-OCSPSigning, another serial, tryLater,
and a delegate without ocsp-nocheck plus the CRLs that clear or revoke it).

`leaf.pfx` password: `pdfcer`. The leaf is issued by `ca.cer`; the pfx
carries the leaf key, the leaf and `ca.cer`.
