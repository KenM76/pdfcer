# fixtures/synthetic/crl — provenance

Synthetic. Minted by `tools/gen-crl-fixtures.py` with pyca/cryptography
(Apache-2.0/BSD; generator-side only, never linked). Every key is random per
run and discarded; every identity names itself a test fixture
("trust nothing"). Re-running replaces the corpus — the committed files are
the fixture. The generator's docstring lists each file and what it
exercises (RFC 5280 §5 CRLs: good, revoked before/after 2026-09-30, stale,
unknown critical extension, delta, forged, other issuer, issuing
distribution point scopes; a CA with and without `cRLSign`).

`leaf.pfx` password: `pdfcer`. The leaf is issued by `ca.cer`; the pfx
carries the leaf key, the leaf and `ca.cer`.
