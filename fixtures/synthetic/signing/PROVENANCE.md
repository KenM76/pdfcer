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

## Added in `Pass 10.14` (2026-09-06)

| File | Key | Container | Exercises |
|---|---|---|---|
| `rsa2048-nolocalkeyid.pfx` | the **same** RSA-2048 key + cert as `rsa2048-modern.pfx` | PBES2 / AES-256-CBC key bag, cert bag in PLAINTEXT `data` (`-certpbe NONE`), MAC SHA-256 — then every `localKeyId` bag attribute REMOVED and the MAC recomputed by the generator (`strip_local_key_id`, Appendix B.2 KDF ported to Python) | `Pkcs12Signer::from_der`'s fallback: key and leaf paired by PUBLIC-KEY IDENTITY when no `localKeyId` pairs them (`P12-6` second clause) |
| `ecp384-modern.pfx`, `ecp384.cer`, `ecp384.key.der` | EC P-384 (`secp384r1`) | PBES2 / AES-256-CBC, MAC SHA-256 | the ECDSA P-384 / SHA-384 signing path end to end (was recorded UNTESTED by the 438th filing for want of a store) |
| `foreign-pyhanko-first.pdf` | signed by **pyHanko** with `rsa2048-modern.pfx` | — | pdfcer adds an approval signature ON TOP of a signature it did not write; both verify, the foreign `/ByteRange` intact |
| `pdfcer-then-pyhanko.pdf` | `pdfcer sign` (field `Signature1`), then pyHanko countersigns (field `ForeignSig`) | — | a foreign tool countersigning pdfcer's output; both verify |

The two `.pdf` fixtures come from `tools/gen-foreign-signature-fixtures.py`
(pyHanko is a generator-side tool only — never linked, rule 13; it stamps the
signing time from the clock, so the bytes are not reproducible and the
committed file is the fixture). `hello.pdf` is pdfcer's own synthetic page.
`gen-signing-fixtures.py` now adds the two stores from the EXISTING
`rsa2048` material by default; `--regen` re-mints everything.

## Added in `Pass 10.13` (2026-09-06) — pre-placed EMPTY signature fields

`tools/gen-sig-field-fixtures.py`, deterministic, nothing signed. One page,
an empty merged `/FT /Sig` field `SignHere` (`/Rect [72 600 300 660]`) and a
text field `Name`:

| File | Extra on the field | Exercises |
|---|---|---|
| `sig-field-empty.pdf` | — | sign INTO the field (its rect/page place the appearance); `Name` for the wrong-type refusal |
| `sig-field-lock.pdf` | `/Lock << /Action /All >>` (Table 233) | the `/FieldMDP` reference copied from the lock (§12.8.2.4, Table 256) |
| `sig-field-lock-include.pdf` | `/Lock << /Action /Include /Fields [(Name)] >>` | same, with `/Fields` |
| `sig-field-sv-ok.pdf` | `/SV` with every evaluable constraint, satisfiable by defaults (`Reasons` recommended only, `MDP /P 0`) | the seed-value evaluator's honoured/noted branches |
| `sig-field-sv-strict.pdf` | `/SV /Ff 72 /Reasons [(Only this reason)] /DigestMethod [/SHA1]` | a REQUIRED constraint a default request violates → refused by name |
| `sig-field-sv-cert.pdf` | `/SV /Cert << /Ff 1 /Subject [(anyone)] >>` (Table 235) | a constraint pdfcer does not evaluate → refused by name, never skipped |
