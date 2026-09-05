# `fixtures/synthetic/signing/` — provenance

**Category (a) under `docs/LEGAL.md` §5: wholly synthetic key material,
minted by a committed script with OpenSSL.** No real person, organisation or
CA is named; the certificate subjects say so in their own `CN`
(*"… (test fixture, trust nothing)"*). Nothing here is trusted by anything,
and nothing here may ever sign a document anyone relies on.

Generator: `tools/gen-signing-fixtures.py`. Regenerate with
`python tools/gen-signing-fixtures.py` from the repository root (needs
`openssl` on PATH; written against 1.1.1s, detects 3.x and adjusts flags).
`--check` exits 1 if a listed file is missing.

## What each file is for

| File | Key | Container encryption | Exercises |
|---|---|---|---|
| `rsa2048-modern.pfx` | RSA-2048 | PBES2 / PBKDF2 / AES-256-CBC, MAC SHA-256 | the modern PKCS#12 shape (RFC 7292 Appendix B recommendation; `P12-9`) |
| `rsa2048-legacy.pfx` | **same** RSA key + cert | `pbeWithSHAAnd3-KeyTripleDES-CBC` key bag, `pbeWithSHAAnd40BitRC2-CBC` cert bags, MAC SHA-1 | the installed-base legacy shape an importer must also read (`P12-10`) — and, because it wraps the same material, a test that both eras decrypt to identical bytes |
| `ecp256-modern.pfx` | EC P-256 | PBES2 / AES-256-CBC | the ECDSA signing path |
| `rsa2048.cer`, `ecp256.cer` | — | DER X.509, plaintext | byte-for-byte equality of the chain pdfcer extracts |
| `rsa2048.key.der`, `ecp256.key.der` | PKCS#8 `PrivateKeyInfo`, plaintext | — | the **OpenSSL oracle only** (`openssl cms -verify` / `-sign` against pdfcer's output). Tests never load these through pdfcer. |

Password for every container: `pdfcer` (ASCII, so the BMPString question of
`P12-11` has one answer for the MAC and the bags).

## Why the keys are not deterministic, and why that is acceptable

`openssl req -newkey` draws a fresh key each run, so regenerating replaces
every committed byte. No test asserts a specific signature value — they
assert round trips (pdfcer signs → pdfcer **and** OpenSSL verify), chain
equality against the `.cer` beside the store, and refusals — so the material
may change without a test changing. Validity is ~100 years (`-days 36500`) so
no test acquires an expiry date. Regenerate only to add a shape, and record
it here.

## Why OpenSSL produced these rather than pdfcer

pdfcer has no PKCS#12 writer (import only — `security__pkcs12_import.md` §0),
so an independent producer is the only option — and that independence is what
makes the files an oracle for the importer rather than a mirror of it
(project rule 7: a fixture must not inherit a bug from the code it tests).
