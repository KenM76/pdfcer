# Signing-crate survey — PAdES B-B authoring dependencies

**Date:** 2026-09-05. **Scope:** crates needed to ADD digital signing (PAdES B-B;
CMS `SignedData`, `/SubFilter /ETSI.CAdES.detached`; RSA PKCS#1 v1.5 + RSASSA-PSS;
ECDSA P-256/P-384; SHA-256; key + chain from PKCS#12 `.pfx`) to `pdfcer-core`,
which already verifies in-crate (`asn1.rs` 293, `cms.rs` 499, `crypto/bignum.rs`
548, `crypto/rsa.rs` 285, `crypto/ecdsa.rs` 435 — verify-only, NOT constant-time,
barred from private-key use by `ARCHITECTURE.md` §12 decision 129).
**Extends** `docs/PRIOR_ART.md` § Cryptography (rows `rsa`, `cms`, `x509-cert`,
`num-bigint`, `p256`/`p384`, `ring`, `pkcs7`, dated ≤ 2026-09-03); does not repeat
them. Nothing in the repo was edited by this survey; no `Cargo.toml` touched.

**Method.** `cargo info <crate>` (cargo 1.97.1, 2026-09-05) for version/licence/
features; crates.io API (`/api/v1/crates/<name>`) for release dates; GitHub API for
issue/PR state; a throw-away Cargo workspace under `%TEMP%\...\scratchpad\sigsurvey`
(deleted after) with one member per candidate group, measured by
`cargo tree -e normal --prefix none | sort -u` (unique `name vX.Y.Z`, proc-macro
crates included), `cargo check --target wasm32-unknown-unknown`, and an `unsafe`
census over the vendored sources in `~/.cargo/registry/src` (non-comment lines
containing the token `unsafe`, minus `forbid/deny(unsafe_code)` lint lines; `src/`
only, so `#[cfg(test)]` blocks are counted). `cargo geiger` and `cargo audit` are
NOT installed on this machine — the census is grep-based; treat counts as an upper
bound on sites, exact on carrier-vs-clean.

Toolchain facts used: `rustc 1.97.1`, `wasm32-unknown-unknown` target installed;
`pdfcer-core` is `#![forbid(unsafe_code)]` (`src/lib.rs:66`); `pdfcer-core`
`Cargo.toml` already carries `aes 0.9.2`, `cbc 0.2.1`, `sha2 0.11.0`, and
`getrandom 0.2` **target-gated off wasm32** (`[target.'cfg(not(target_arch =
"wasm32"))'.dependencies]`, line 369; `crypto/rng.rs` returns `RngError::Unavailable`
on wasm32).

---

## 0. Headline findings (read these before the tables)

| # | Finding | Consequence |
|---|---|---|
| 1 | **`cms` `builder` feature does not compile** — neither `0.3.0-pre.1` nor `0.3.0-pre.2`, native or wasm32 — against today's resolution (`cipher 0.5.2`, `elliptic-curve 0.14.1`): 11 errors (`unresolved import cipher::crypto_common`, `Array::generate_from_rng` missing, `PublicKey::to_encoded_point` missing). The Jan-2026 pre-releases predate the Jun/Jul-2026 finals of their own deps. Types-only `cms` (no `builder`) compiles on both targets. | `SignedDataBuilder` is **unusable today**. CMS assembly is written by pdfcer (on `der` types) regardless of which stack is chosen. |
| 2 | **`pkcs12 0.2.0-pre.0` pins `cms = "=0.3.0-pre.1"`** (cargo resolver: *"versions that meet the requirements `=0.3.0-pre.1`"*), so it **cannot coexist with `cms 0.3.0-pre.2`** in one lockfile. Upstream `master` already says `cms = "0.3.0-pre.2"` — a `pkcs12 0.2.0-pre.1` is pending but unreleased. | Anyone taking `pkcs12` today is pinned to `cms pre.1` and will be forced to move when `pkcs12` re-releases. |
| 3 | **`pkcs12` parses and derives keys but does not decrypt**: `src/lib.rs:108 "// todo: add decryption support"`, `"// todo: add RC2 support"`; `MacData` is a plain struct with no verify fn. **`pkcs5 0.8.1` implements NO PBES1 decryption** (`Error::NoPbes1CryptSupport` unconditionally, `src/lib.rs:91/110/124/144`) and its PBES1 enum holds only RFC 8018 A.3 OIDs (MD2/MD5/SHA1 × DES/RC2) — the PKCS#12 PBE OIDs (`1.2.840.113549.1.12.1.3` 3-key-3DES, `.1.6` 40-bit-RC2) are absent. | Legacy `.pfx` decryption (every Windows export, every OpenSSL < 3 export) and MAC verification are **hand-assembled either way**: `pkcs12::kdf::derive_key` + `des`/`rc2` + `cbc` + `hmac`. `pkcs12` buys the KDF (≈80 lines) and the PFX struct types. |
| 4 | **`rsa 0.10.0-rc.18`** (2026-04-27) is still a release candidate; **RUSTSEC-2023-0071 still lists "no patched versions"** (advisory last modified 2026-04-25). Issue #19 (modpow not constant-time) **closed 2026-01-07** after the `crypto-bigint 0.7` migration; the same day issue **#626 "Padding implementation is not constant-time" opened** (still open, 11 comments) — the residual channel is the **PKCS#1 v1.5 *decryption* de-padding oracle**, fixed by PR **#680 "implicit rejection"** (open, unmerged, created 2026-04-01). On 2026-06-01 a contributor demonstrated an end-to-end Bleichenbacher attack against rc.18 *decrypt* that fails against #680. | **Signing is not a decryption oracle**: EMSA-PKCS1-v1_5 encoding of a digest is public and deterministic; the secret enters only in the modexp, which is now `crypto-bigint` constant-time + optional blinding. The advisory's own workaround — *"avoid ... where attackers can observe timing data (local, non-compromised systems are considered safe)"* — describes a desktop signing operation exactly. The exposure is documented, not zero. |
| 5 | **`rsa`'s non-randomized `Signer`/`DigestSigner`/`PrehashSigner` impls for PKCS#1 v1.5 pass `rng = None` ⇒ NO blinding** (`pkcs1v15/signing_key.rs:96–125,176–195` → `sign::<DummyRng>(None, ..)`). Blinding happens only via `RandomizedSigner`/`RandomizedPrehashSigner`. PSS has no non-randomized `Signer` at all unless the `getrandom` feature is on (`pss/signing_key.rs:165–185`, `#[cfg(feature = "getrandom")]` → `SysRng`). | pdfcer must call the `Randomized*` paths with its own `rand_core 0.10` `TryCryptoRng` adapter over `crypto::rng::fill` (≈20 lines). Do **not** enable `rsa/getrandom`: it adds `getrandom 0.4.3`, which **fails on wasm32 without `wasm_js`** (measured), and would be a second `getrandom` beside pdfcer's 0.2. |
| 6 | **No second `sha2`/`digest` is introduced.** The whole candidate stack resolves to `sha2 0.11.0` / `digest 0.11.3` / `signature 3.0.0` / `rand_core 0.10.1`; `cargo tree -d` on the full member printed *"nothing to print"*. (pdfcer's lock already carries `sha2 0.10.9` via `pdfcer-fetch` — pre-existing, unrelated.) | Question (E) answered: `rsa`, `p256`, `pkcs12`, `pkcs5`, `pbkdf2`, `hmac` all take `digest 0.11`. |
| 7 | **`der 0.8.2` was published TODAY (2026-09-05)** and `der 0.8.0` (2026-02-13) was **yanked today** — changelog: *"yanked to fix a `minimal-versions` check in CI"*; 0.8.1 (2026-07-08) added `SetOfRef` and fixed *"SET OF is not supposed to disallow duplicates"*; 0.8.2 fixed *"Return error on nested trailing data"*. | `der 0.8` is stable-track but actively patched; pin `>=0.8.1`. `der` enters the tree **anyway** through `rsa/encoding` and `p256/pkcs8`, so using it as pdfcer's DER *encoder* costs zero extra crates. |
| 8 | The 2026 RustCrypto **stable wave landed Jul 2026**: `p256`/`p384 0.14.0` (2026-07-03), `ecdsa 0.17.0` (2026-07-02), `x509-cert 0.3.0` (2026-07-09), `pkcs8 0.11.0`, `pkcs5 0.8.1`, `spki 0.8.0`, `signature 3.0.0`, `hmac 0.13.0`, `pbkdf2 0.13.0`, `des 0.9.0`, `rc2 0.9.0`, `sha1 0.11.0`. Still pre-release: **`rsa 0.10.0-rc.18`, `cms 0.3.0-pre.x`, `pkcs12 0.2.0-pre.0`, `pkcs1 0.8.0-rc.4`** (the last pulled transitively by `rsa`). | The ECDSA half is fully stable. The RSA half and the PKCS#12/CMS half carry pre-release pins. |

---

## 1. Version / licence / status table (all `cargo info`, 2026-09-05)

| Crate | Version | Released | Licence | Class (rule 13) | MSRV | Pre-release? | Notes |
|---|---|---|---|---|---|---|---|
| `rsa` | 0.10.0-rc.18 | 2026-04-27 | MIT OR Apache-2.0 | permissive | 1.85 | **yes (rc)** — max_stable 0.9.10 (2026-01-06, `num-bigint-dig`, NOT constant-time) | features: `default=[std,encoding]`, `encoding=[pkcs1,pkcs8,spki]`, `sha2`, `sha1`, `getrandom`, `hazmat`, `pkcs5=[pkcs8/encryption]`, `serde`. deps: `crypto-bigint 0.7`, `digest 0.11`, `pkcs8 0.11`, `rand_core 0.10`, `signature 3.0.0-rc.10` (→3.0.0), `zeroize 1.8` |
| `p256` | 0.14.0 | 2026-07-03 | Apache-2.0 OR MIT | permissive | 1.85 | no | `ecdsa=[arithmetic, ecdsa-core/algorithm, sha256]`, `pkcs8`, `pem`, `getrandom`, `std` |
| `p384` | 0.14.0 | 2026-07-03 | Apache-2.0 OR MIT | permissive | 1.85 | no | `ecdsa=[arithmetic, ecdsa-core/algorithm, sha384]` |
| `ecdsa` | 0.17.0 | 2026-07-02 | Apache-2.0 OR MIT | permissive | 1.85 | no | RFC 6979 deterministic `k` on `DigestSigner`/`PrehashSigner`/`Signer` (`signing.rs:145–190`); `Randomized*` = RFC 6979 §3.6 hedged (additional data); `hazmat::sign_prehashed_rfc6979` |
| `signature` | 3.0.0 | 2026 | Apache-2.0 OR MIT | permissive | 1.85 | no | trait crate; `digest` feature |
| `elliptic-curve` | 0.14.1 | 2026 | Apache-2.0 OR MIT | permissive | 1.85 | no | `SecretKey: TryFrom<pkcs8::PrivateKeyInfoRef>` (`secret_key/pkcs8.rs:46`) |
| `rfc6979` | 0.6.0 | — | Apache-2.0 OR MIT | permissive | — | no | via `ecdsa` |
| `pkcs12` | 0.2.0-pre.0 | 2026-01-12 | Apache-2.0 OR MIT | permissive | 1.85 | **yes (pre)** — max_stable 0.1.0 (2024-01-04, `der 0.7` generation) | `default=[pem]`, `kdf=[digest, zeroize/alloc]`, `zeroize`. deps `der 0.8, spki 0.8, x509-cert 0.3, const-oid 0.10, cms =0.3.0-pre.1 (published) / 0.3.0-pre.2 (master)` |
| `pkcs8` | 0.11.0 | 2026 | Apache-2.0 OR MIT | permissive | 1.85 | no | `encryption`→`pkcs5` PBES2 decrypt of `EncryptedPrivateKeyInfo`; `3des`, `des-insecure`, `pem`, `getrandom` |
| `pkcs5` | 0.8.1 | 2026 | Apache-2.0 OR MIT | permissive | 1.85 | no | `pbes2=[aes, cbc, pbkdf2, scrypt, sha2, aes-gcm]`, `3des=[des,pbes2]`, `des-insecure`, `sha1-insecure=[sha1,pbes2]`, `getrandom`. **PBES1 = parse-only** |
| `des` | 0.9.0 | 2026 | MIT OR Apache-2.0 | permissive | 1.85 | no | `TdesEde3` for 3-key-3DES |
| `rc2` | 0.9.0 | 2026 | MIT OR Apache-2.0 | permissive | 1.85 | no | `Rc2::new_with_eff_key_len(key, eff_bits)` (`lib.rs:46`) — needed for 40-bit effective key |
| `cbc` | 0.2.1 | — | MIT OR Apache-2.0 | permissive | 1.85 | no | **already in pdfcer-core** |
| `hmac` | 0.13.0 | 2026 | MIT OR Apache-2.0 | permissive | 1.85 | no | PKCS#12 MAC, PBKDF2 PRF |
| `pbkdf2` | 0.13.0 | 2026 | MIT OR Apache-2.0 | permissive | 1.85 | no | `default-features=false, features=["hmac"]` |
| `sha1` | 0.11.0 | 2026 | MIT OR Apache-2.0 | permissive | 1.85 | no | needed as a `digest::Digest` impl for `pkcs12::kdf::derive_key::<Sha1>` (legacy PBE + SHA-1 MAC); in-house `crypto/sha1.rs` has no `Digest` impl |
| `sha2` | 0.11.0 | — | MIT OR Apache-2.0 | permissive | 1.85 | no | **already in pdfcer-core** at 0.11.0 |
| `digest` | 0.11.3 | — | MIT OR Apache-2.0 | permissive | 1.85 | no | transitively |
| `der` | 0.8.2 | **2026-09-05** | Apache-2.0 OR MIT | permissive | 1.85 | no (0.8.0 yanked 2026-09-05; 0.8.1 2026-07-08) | `alloc`, `oid`, `derive` (+`der_derive 0.8.0` proc-macro), `pem`, `std` |
| `cms` | 0.3.0-pre.2 / -pre.1 | 2026-01-25 / 2026-01-12 | Apache-2.0 OR MIT | permissive | 1.85 | **yes (pre)** — max_stable 0.2.3 (2024-01-08, `der 0.7`) | `builder` **broken** (finding 1); types-only compiles |
| `x509-cert` | 0.3.0 | 2026-07-09 | Apache-2.0 OR MIT | permissive | 1.85 | no | `builder`, `digest`, `hazmat`, `pem`, `std` |
| `spki` | 0.8.0 | 2026 | Apache-2.0 OR MIT | permissive | 1.85 | no | transitively |
| `const-oid` | 0.10.2 | — | Apache-2.0 OR MIT | permissive | 1.85 | no | transitively |
| `crypto-bigint` | 0.7.5 | 2026 | Apache-2.0 OR MIT | permissive | — | no | `rsa`'s constant-time bignum (`BoxedUint`, `BoxedMontyParams`) |
| `p12-keystore` | 0.3.1 | 2026-06-19 | MIT/Apache-2.0 | permissive | 1.85 | no (0.3.0 2026-06-07; rc3/rc4 Feb–Apr 2026) | ancwrd1; 62 commits; 8.2 M downloads. deps: `pkcs12 0.2.0-pre.0 [kdf]`, `cms 0.3.0-pre.1`, `pkcs5 0.8 [pbes2]`, `pkcs8 0.11`, `der 0.8 [std,derive]`, `x509-parser 0.18`, `rand 0.10`, `thiserror 2`, `hex`, `cbc/rc2/des` (feature `pbes1`, default on), `sha1/sha2/hmac 0.11/0.13` |
| `p12` | 0.6.3 | **2022-02-18** | MIT OR Apache-2.0 | permissive | 1.56 | no | hjiayz; **no release in 3.5 years**; `yasna 0.5`, `des 0.8`, `rc2 0.8`, `cbc 0.1`, `cipher 0.4`, `hmac 0.12`, `sha1 0.10`, `getrandom 0.2` (unconditional), `lazy_static` — the **previous RustCrypto generation** (`digest 0.10`): would add a second `digest`/`sha1`/`cipher`/`hmac` line to the lock |

RustSec: `rustsec.org/packages/rsa.html` exists (RUSTSEC-2023-0071 only);
`/packages/{der,ecdsa,elliptic-curve,crypto-bigint,pkcs12,p256}.html` all **404**
(RustSec only creates a package page when an advisory exists) ⇒ no advisories on
those six as of 2026-09-05.

---

## 2. (A) RSA private-key signing — `rsa 0.10.0-rc.18`

**Advisory state (fetched 2026-09-05).**
`https://rustsec.org/advisories/RUSTSEC-2023-0071.html`: *"Marvin Attack: potential
key recovery through timing sidechannels"*; affected: all versions; **patched: none**;
issued 2023-11-28, **last modified 2026-04-25**; CVSS 5.9 MEDIUM; *"Work is
underway to migrate to a fully constant-time implementation"*; workaround: *"Avoid
using the crate in environments where attackers can observe timing data (local,
non-compromised systems are considered safe)"*; refs: RSA#626, RSA#19, CVE-2023-49092.

**README shipped in rc.18** (`docs.rs/crate/rsa/0.10.0-rc.18` and vendored
`README.md:62–74`): the old sentence *"the implementation of modular exponentiation
is not constant time, but timing variability is masked using random blinding"* is
**struck through** (`~~...~~`), followed by *"This crate is vulnerable to the Marvin
Attack which could enable private key recovery by a network attacker (see
RUSTSEC-2023-0071). You can follow our work on mitigating this issue in #390."*
Status block: *"Currently at Phase 1 🚧 — Make it work"* of three phases to 1.0;
one Include Security audit (2019, one minor finding fixed). `CHANGELOG.md` in the
crate **stops at 0.9.8 (2025-03-12)** — the 0.10 rc line has no changelog entries;
GitHub Releases page is empty; tags show `v0.10.0-rc.11` (2026-01-03) … `rc.18`
(2026-04-27, "pkcs8 v0.11 updates"), `rc.16` (2026-02-27, "crypto-bigint
compatibility"), and **no `v0.10.0` final**.

**Upstream tracking (GitHub API, 2026-09-05).**
- **#19** *"modpow implementation is not constant-time"* — **closed 2026-01-07**,
  97 comments, label `security`. 2025 thread: rc.0 shipped the `crypto-bigint`
  migration (May–Jun 2025); a contributor's 1 M-iteration test on rc.3 found *"the
  obvious vulnerability doesn't appear anymore"* but a residual channel at
  p = 2.08 × 10⁻⁶; maintainer (tarcieri): *"not confident it completely addresses
  the issue"*; Dec 2025: `ctutils` crate with inline-asm CMOV/CSEL replaces `subtle`
  for guaranteed constant-time selects (this is why `ctutils 0.4.2` + `cmov 0.5.4`
  with 3 `asm!` files are in the tree).
- **#390** *"Migrating from num-bigint(-dig) to crypto-bigint"* — **closed**
  (the README still points at it).
- **#626** *"Padding implementation is not constant-time"* — **open**, created
  2026-01-07 (same day #19 closed), 11 comments. Body: side-channels *"may no longer
  stem from the underlying bigint library, but rather from padding mode
  implementations"*. 2026-03-24 Kani formal-verification harness proving the leak
  and the implicit-rejection fix; 2026-05-03 10 M-sample timing run inconclusive;
  2026-05-14 (tomato42): cannot exclude channels > 3.5 ns without ≈15× more data;
  **2026-06-01 (eslerm): end-to-end Bleichenbacher attack succeeds against rc.18
  `decrypt`, fails against #680**; same author's triage: *modexp timing addressed;
  behavioural (decryption) oracle closed by #680; blinding on the default path still
  open*.
- **#680** *"Implicit rejection api for PKCS#1 v1.5 decryption"* — **open,
  unmerged**, created 2026-04-01.

**What this means for SIGNING specifically.** Marvin/Bleichenbacher is a
*decryption* padding-oracle attack: the attacker submits chosen ciphertexts and
times the de-padding. Signing has no de-padding step — EMSA-PKCS1-v1_5 (RFC 8017
§9.2) and EMSA-PSS encoding operate on the public digest — so #626/#680 do not
reach the signing path. What does reach it is the classic Kocher timing attack on
the private modexp itself, which (i) `crypto-bigint 0.7`'s `BoxedMontyParams`
constant-time `pow_mod_params` addresses (#19 closed on that basis), (ii) blinding
further masks, and (iii) requires an attacker able to trigger many signatures with
chosen inputs and observe timing — not the shape of an operator clicking *Sign* in a
desktop app. **Blinding is opt-in by API** (finding 5): `RsaPrivateKey::sign` and all
non-`Randomized*` trait impls pass `None` for the RNG and skip `blind()`
(`algorithms/rsa.rs:37–61,174–216`); `RandomizedSigner::try_sign_with_rng` /
`RandomizedPrehashSigner::sign_prehash_with_rng` blind. **Use the `Randomized*`
paths.**

**API surface pdfcer needs (rc.18, vendored source).**
- Import: `RsaPrivateKey: TryFrom<pkcs8::PrivateKeyInfoRef<'_>>` (`encoding.rs:171`,
  feature `encoding`); `pkcs1v15::SigningKey<D>` / `pss::SigningKey<D>` /
  `pss::BlindedSigningKey<D>` also `TryFrom<PrivateKeyInfoRef>`.
- PKCS#1 v1.5: `pkcs1v15::SigningKey::<Sha256>::new(key)`;
  `RandomizedPrehashSigner::sign_prehash_with_rng(&mut rng, digest)` (blinded).
  `signature_algorithm_identifier()` yields `sha256WithRSAEncryption`
  (`1.2.840.113549.1.1.11`) — matches pdfcer's `cms.rs` `SHA256_WITH_RSA`.
- PSS: `pss::SigningKey::<Sha256>::new(key)` (salt = digest len = 32 — the value
  ETSI EN 319 122 / RFC 4055 verifiers expect); `RandomizedPrehashSigner`;
  `signature_algorithm_identifier()` yields `id-RSASSA-PSS` with `RSASSA-PSS-params`
  (`get_pss_signature_algo_id::<D>(salt_len)` — `pss/signing_key.rs:224`).
- RNG: trait `rand_core 0.10::TryCryptoRng` (`TryRng` + marker). A 20-line adapter
  over `pdfcer_core::crypto::rng::fill` satisfies it; on wasm32 `fill` refuses ⇒
  signing refuses (consistent with encryption authoring, `rng.rs` header).

**Measured (scratch member `m_rsa`: `rsa default-features=false, features=[sha2,
encoding]` + `sha2 0.11` + `signature 3.0`).** **25 unique crates** (0 proc-macro):
`base64ct 1.8.3, block-buffer 0.12.1, cfg-if 1.0.4, cmov 0.5.4, const-oid 0.10.2,
cpubits 0.1.1, cpufeatures 0.3.1, crypto-bigint 0.7.5, crypto-common 0.2.2,
crypto-primes 0.7.2, ctutils 0.4.2, der 0.8.2, digest 0.11.3, hybrid-array 0.4.14,
num-traits 0.2.19, pem-rfc7468 1.0.0, pkcs1 0.8.0-rc.4, pkcs8 0.11.0, rand_core
0.10.1, rsa 0.10.0-rc.18, sha2 0.11.0, signature 3.0.0, spki 0.8.0, typenum 1.20.1,
zeroize 1.9.0`. **wasm32 `cargo check`: OK (rc=0).** Adding `rsa/getrandom`:
+`getrandom 0.4.3`, **wasm32 FAILS** (*"wasm32/64-unknown-unknown are not supported
by default; you may need to enable the `wasm_js` crate feature"*). **`unsafe`
carriers in `m_rsa`: 12** — `base64ct` (4 lines), `block-buffer` (21), `cmov` (25,
3 `asm!` files), `const-oid` (1), `cpufeatures` (11, 1 asm), `crypto-bigint` (14),
`der` (4), `hybrid-array` (37), `num-traits` (1), `pem-rfc7468` (1), `sha2` (53, 4
asm — already accepted, decision 039), `zeroize` (17, 1 asm). `rsa` itself: **0
`unsafe`**; `crypto-primes`, `ctutils` (`forbid(unsafe_code)`), `pkcs1/8`, `spki`,
`signature`, `rand_core`, `digest`: 0.

---

## 3. (B) ECDSA P-256 / P-384 signing — `p256`/`p384 0.14.0` + `ecdsa 0.17.0`

All **stable** (Jul 2026), Apache-2.0 OR MIT, MSRV 1.85, `#![forbid(unsafe_code)]`
on `p384`, `primefield`, `ff`, `wnaf`, `sec1`, `signature`; `p256`, `ecdsa`,
`primeorder`, `group`, `rfc6979` have 0 `unsafe` lines; `elliptic-curve 0.14.1` has
4.

**RFC 6979.** `ecdsa::SigningKey<C>` implements `DigestSigner`, `PrehashSigner`,
`Signer`, `MultipartSigner` with *"a deterministic ephemeral scalar (k) computed
using the algorithm described in RFC 6979 § 3.2"* (`signing.rs:145–200`) — **no RNG
required**, so ECDSA signing works on wasm32 in principle. `Randomized*` variants
feed the RNG output as RFC 6979 §3.6 additional data (hedged; still
deterministic-safe). pdfcer's existing verifier already checks against RFC 6979
A.2.5/A.2.6 vectors (`crypto/ecdsa.rs`), so the signer's output is directly
testable in-crate.

**Import.** `elliptic_curve::SecretKey<C>: TryFrom<pkcs8::PrivateKeyInfoRef>`
(feature `pkcs8`) ⇒ `p256::ecdsa::SigningKey::from(secret_key)`. The PKCS#8 body
from a `.pfx` `pkcs8ShroudedKeyBag` is an `ECPrivateKey` (RFC 5915) under
`id-ecPublicKey` with `namedCurve` — handled by `sec1 0.8.1`.
`signature_algorithm_identifier()` → `ecdsa-with-SHA256` (`1.2.840.10045.4.3.2`) /
`ecdsa-with-SHA384`; signature encoding: `Signature::to_der()` gives the
`ECDSA-Sig-Value SEQUENCE {r, s}` CMS requires (RFC 5753 §2.1.1) — use `to_der()`,
not the fixed-size `to_bytes()`.

**Measured (`m_ecdsa`: `p256`/`p384 default-features=false, features=[ecdsa,pkcs8]`
+ `ecdsa 0.17 default-features=false` + `signature 3.0` + `sha2 0.11`).** **34 unique
crates**, 0 proc-macro; wasm32 OK. New beyond the RSA set: `base16ct 1.0.0, ecdsa
0.17.0, elliptic-curve 0.14.1, ff 0.14.0, group 0.14.0, hmac 0.13.0, p256 0.14.0,
p384 0.14.0, primefield 0.14.0, primeorder 0.14.0, rfc6979 0.6.0, sec1 0.8.1,
subtle 2.6.1, wnaf 0.14.1`. **`unsafe` carriers: 13** (the RSA list minus
`base64ct`/`pem-rfc7468`/`crypto-primes`, plus `base16ct` 4, `elliptic-curve` 4,
`subtle` 2). Shares `crypto-bigint`, `der`, `pkcs8`, `spki`, `cmov`, `ctutils`,
`hybrid-array`, `zeroize` with `rsa` — the union (RSA + ECDSA) is **39 crates**, not
59.

---

## 4. (C) PKCS#12 `.pfx` parsing + decryption

**What a real `.pfx` contains** (RFC 7292; behaviour of the two producers that
matter): outer `PFX { version 3, authSafe ContentInfo(data → OCTET STRING of
AuthenticatedSafe), macData MacData }`; `AuthenticatedSafe = SEQUENCE OF
ContentInfo`, each either `data` (plain `SafeContents`) or `encryptedData` (CMS
`EncryptedData` wrapping `SafeContents`); bags: `keyBag` (plain PKCS#8),
`pkcs8ShroudedKeyBag` (`EncryptedPrivateKeyInfo`), `certBag` (`x509Certificate`
OCTET STRING of DER cert). Encryption schemes seen in the wild:
- **Legacy (OpenSSL ≤ 1.1 default; Windows `certmgr`/`Export-PfxCertificate`
  default; most CA-issued `.pfx`):** key bag `pbeWithSHAAnd3-KeyTripleDES-CBC`
  (`1.2.840.113549.1.12.1.3`), cert bag `pbeWithSHAAnd40BitRC2-CBC`
  (`1.2.840.113549.1.12.1.6`), MAC `HMAC-SHA1`, iterations 2048 — all using the
  **PKCS#12 KDF (RFC 7292 App. B)**, not PBKDF2.
- **Modern (OpenSSL 3.x default):** PBES2 / PBKDF2-HMAC-SHA256 / AES-256-CBC for
  both bags, MAC `HMAC-SHA256`, 2048 iterations — but the MAC key is **still**
  derived with the PKCS#12 KDF (id = 3).
Any importer therefore needs **both** the PKCS#12 KDF (always, for the MAC) and
PBES2 (modern) and PBES1-style 3DES/RC2 (legacy).

**Coverage matrix (vendored sources read 2026-09-05).**

| Need | `pkcs12 0.2.0-pre.0` | `pkcs5 0.8.1` / `pkcs8 0.11` | `p12-keystore 0.3.1` | `p12 0.6.3` | In-house on `asn1.rs` |
|---|---|---|---|---|---|
| PFX / AuthenticatedSafe / SafeBag / CertBag / MacData **types + DER decode** | yes (`pfx.rs`, `authenticated_safe.rs`, `safe_bag.rs`, `cert_type.rs`, `mac_data.rs`, `pbe_params.rs`; `EncryptedData` via `cms`) | — | yes (own codec on `pkcs12` types) | yes (`yasna`) | walker exists; ≈120 lines to add |
| PKCS#12 KDF (RFC 7292 B.2) | **yes**, feature `kdf`: `derive_key_utf8::<D>(pw, salt, Pkcs12KeyType::{EncryptionKey=1,Iv=2,Mac=3}, rounds: i32, key_len) -> der::Result<Vec<u8>>`, `derive_key_bmp`, `derive_key` (`kdf.rs:25–100`) | no | via `pkcs12` | own | ≈60–80 lines |
| MAC verification | **no** — `MacData {mac: DigestInfo, mac_salt, iterations}` struct only | no | yes (own) | yes | ≈20 lines (`hmac`) |
| PBES2 (PBKDF2-HMAC-SHA2 + AES-CBC / 3DES-CBC) decrypt | **no** (`// todo: add decryption support`) | **yes**: `pkcs8 [encryption,3des]` → `pkcs5 pbes2` decrypts `EncryptedPrivateKeyInfo`; `pkcs5::EncryptionScheme::decrypt` for `EncryptedData` params too. Cost: `pbes2` pulls `aes-gcm, scrypt, salsa20, ctr, ghash, polyval, universal-hash, aead` (8 crates no `.pfx` uses) | yes | **no** (3DES/RC2 only) | params walk ≈80 lines + `pbkdf2`+`aes`(have)+`cbc`(have)+`des` |
| PBES1 `pbeWithSHAAnd3-KeyTripleDES-CBC` decrypt | OIDs only (`lib.rs:57`) | **no** — `NoPbes1CryptSupport`; OID not even in enum | **yes** (`pbes1.rs` 68 lines: `derive_key` 24 B key + 8 B IV → `cbc::Decryptor<TdesEde3>`) | yes | ≈40 lines with `des`+`cbc` |
| PBES1 `pbeWithSHAAnd40BitRC2-CBC` decrypt | OIDs only (`"todo: add RC2 support"`) | no | **yes** (`Rc2` 5-byte key, eff 40 bits) | yes | ≈40 lines with `rc2`+`cbc` |
| wasm32 | yes (types+kdf) | yes | **no** — `rand 0.10`→`getrandom 0.4.3` (measured fail) | **no** — `getrandom 0.2` unconditional (measured fail) | yes |
| `unsafe` (own) | 0, `forbid` | 0, `forbid` | 0 | 0 | 0 (`forbid` in crate) |
| Pre-release pin | **yes**, and pins `cms =0.3.0-pre.1` | no | inherits `pkcs12 pre.0` + `cms pre.1` | no (stale 2022) | n/a |
| Unique crates (measured) | `m_pkcs12` (pkcs12+pkcs8[encryption,3des]+pkcs5[pbes2,3des]+des+rc2+cbc+hmac+pbkdf2+sha1+sha2): **47** (5 proc-macro), **19 unsafe carriers** | — | `m_p12ks`: **78** (12 proc-macro; adds `x509-parser`, `asn1-rs`, `nom`, `time`, `thiserror`, `syn 3.0.5`) | `m_p12`: **20**, but the `digest 0.10` generation (second `sha1`/`hmac`/`cipher`/`des`/`cbc` lines) | 0 beyond the ciphers |

**Verdicts.** `p12` (2022): skip — stale, second RustCrypto generation, unconditional
`getrandom 0.2` breaks wasm32. `p12-keystore`: not as a dependency (78 crates,
`x509-parser`/`nom`/`time` duplication of what `asn1.rs` already does, wasm32 fail),
**but its `src/pbes1.rs` (68 lines) + `codec.rs` are a clean MIT/Apache-2.0
reference implementation of exactly the legacy path pdfcer must write** — read it,
credit it in the doc comment if structure is borrowed. `pkcs12`: takes types +
KDF, still leaves decryption and MAC to pdfcer, and drags `cms =pre.1` +
`x509-cert` + `der_derive`; since `asn1.rs` already walks DER and the KDF is ≈80
lines, the **lean route (in-house PFX walker + KDF on `asn1.rs`, ciphers from
crates)** costs fewer crates and no pre-release pin. `pkcs8 [encryption]` is a
reasonable middle ground for PBES2 only, but its `pbes2` feature's 8 unused
AEAD/scrypt crates argue for hand-parsing PBES2 params (PBKDF2 params +
`aes256-CBC`/`des-ede3-CBC` AlgorithmIdentifier — ≈80 lines) instead.

Legacy note for the implementation: the PKCS#12 KDF password is **BMPString**
(UTF-16BE) **with a trailing 0x0000** (`pkcs12::kdf::derive_key_bmp` appends
`[0u8; 2]`); RC2-40 uses a 5-byte key with `eff_key_len = 40` — `Rc2::new(key)`
alone sets effective bits = key bits, which happens to equal 40 for a 5-byte key,
but call `new_with_eff_key_len(key, 40)` explicitly. MAC input is the **content
octets** of the `authSafe` `data` OCTET STRING, not the whole `ContentInfo`.

---

## 5. (D) DER / CMS building

**`cms` builder, as read in `cms-0.3.0-pre.1/src/builder.rs` (1 288 lines) — the
API it *would* offer if it compiled:** `SignerInfoBuilder::new(sid, digest_algorithm,
&EncapsulatedContentInfo, external_message_digest: Option<&[u8]>)` — **detached
content supported** (RFC 5652 §5.2: `eContent` must be `None` when an external
digest is given, `builder.rs:139–146,197–212`); `add_signed_attribute`,
**`add_unsigned_attribute`** (`:169–174`, so an RFC 3161 `id-aa-signatureTimeStampToken`
could be attached — but only at build time; post-signing insertion means
re-encoding the `SignerInfo`, which the `der` `Sequence` types permit);
auto-generates `content-type` (checking it matches `eContentType`, `:252–279`) and
`message-digest`; **signature is computed over
`SignedAttributes::encode_to_vec()` where `SignedAttributes = Attributes =
SetOfVec<Attribute>`** — encoding a `SET OF` emits tag **0x31** with DER-sorted
elements, so **the RFC 5652 §5.4 "retag IMPLICIT [0] → SET" is inherent** (the `[0]`
only appears when the enclosing `SignerInfo` is encoded); signer bound
`S: Keypair + DynSignatureAlgorithmIdentifier` + `signature::Signer<Sig>` /
`RandomizedSigner` / async variants (`add_signer_info`, `_with_rng`, `:406–495`);
`SignedDataBuilder::{new(&EncapsulatedContentInfo), add_digest_algorithm,
add_certificate(CertificateChoices), add_crl, build() -> ContentInfo}`.
**Status: does not compile (finding 1). Do not plan on it.**

**`cms` types-only (no `builder`)**: compiles native + wasm32; `m_cms` (`cms
=0.3.0-pre.1 [std]` + `x509-cert 0.3.0` + `der 0.8.2 [alloc,derive,oid]`) = **11
unique crates**: `cms, const-oid, der, der_derive, flagset, proc-macro2, quote, spki,
syn, unicode-ident, x509-cert` (5 of them proc-macro/build-time; `proc-macro2`,
`quote`, `syn 2.0.119`, `unicode-ident` are **already in pdfcer's lock**).
**`unsafe` carriers: 6** (`der` 4 lines, `const-oid` 1, `flagset` 2, and the three
proc-macro helpers). It supplies `SignedData`, `SignerInfo`, `SignerIdentifier`,
`EncapsulatedContentInfo`, `CertificateSet`, `Attribute(s)`, `ContentInfo` as
`der::Sequence` types with `Encode`. What it *costs*: a pre-release pin whose exact
value is dictated by whether `pkcs12` is also taken (`=pre.1`) and which will move
when `cms 0.3.0` finals.

**In-house writer on top of `asn1.rs`.** Current `asn1.rs` is a 293-line
`pub(crate)` **reader** (`Tlv`, `read`, `expect`, `children`, `oid_to_string`,
`integer_bytes`, `bit_string_bytes`, `string_value`, `time_value`; tag consts incl.
`SET = 0x31`), whose header says *"Not an encoder"*. A writer sufficient for PAdES
B-B `SignedData` needs: TLV with definite long-form lengths; `INTEGER` (minimal
two's-complement, leading-0x00 rule — for `version`, `serialNumber`); `OID` from
dotted string (or pre-baked byte consts — ≈12 OIDs); `OCTET STRING`; `NULL`;
`SEQUENCE`; **`SET OF` with X.690 §11.6 ordering of the encoded attributes**
(sort by encoded bytes — the one subtle rule; 3–4 attributes); `UTCTime`
(signing-time is *optional* in PAdES B-B and ETSI discourages it — cf. EN 319
122-1 — but Acrobat displays it, so emit it); context-specific `[0]` (certificates,
signedAttrs) and `[3]` (unsignedAttrs) IMPLICIT wrappers; `BIT STRING` is not
needed (CMS `signature` is an `OCTET STRING`; certificates are passed through as
raw DER from the `.pfx`). Plus the PAdES-mandatory **ESS `signing-certificate-v2`**
attribute (`1.2.840.113549.1.9.16.2.47`, `ESSCertIDv2 { hashAlgorithm DEFAULT
sha256 (omit), certHash OCTET STRING, issuerSerial OPTIONAL }`). **Estimate: 250–400
lines + tests**, oracle = the existing `cms.rs` parser + `signature_verify.rs`
(independent implementation → a real round-trip test), plus `openssl cms -verify
-binary -inform DER` / `asn1parse` in CI. Risk: X.690 §11.6 ordering bugs surface
only in strict verifiers (Adobe is strict; `openssl` is lenient) — test against
the sorted output of `der`'s `SetOfVec` in a unit test if `der` is present anyway.

**`der 0.8.2` as the encoder (no `cms`).** `der` is **already in the tree via
`rsa/encoding` and `p256/pkcs8`** (`der`, `spki`, `pkcs8`, `const-oid`,
`pem-rfc7468`, `base64ct` appear in `m_rsa` and `m_ecdsa`). Using its `Encode`,
`SequenceOf`/`SetOfVec` (DER ordering built in), `ObjectIdentifier`,
`OctetStringRef`, `UtcTime`, `ContextSpecific`, `Any` to build the CMS structures
costs **zero additional crates** without `derive`, or **+5 build-time crates**
(`der_derive 0.8.0` + `proc-macro2`/`quote`/`syn`/`unicode-ident`, four of which
pdfcer's lock already has) with `derive`. `der` has 4 `unsafe` lines (0.8.2), is
stable-track, and is the same type system `rsa`/`p256` hand back
`AlgorithmIdentifier`s in (`DynSignatureAlgorithmIdentifier` →
`spki::AlgorithmIdentifierOwned`, a `der` type) — so the `signatureAlgorithm` field
of `SignerInfo` is a direct paste rather than a re-encode.

**Verdict (D).** `cms` buys nothing today that `der` alone does not (its builder is
broken; its types are ≈150 lines of `#[derive(Sequence)]` structs pdfcer can declare
itself with **stable** version pins). Preferred: **`der 0.8` (already present) +
pdfcer-declared CMS structs** — hand-declare `SignedData`, `SignerInfo`,
`EncapsulatedContentInfo`, `Attribute`, `IssuerAndSerialNumber`, `ESSCertIDv2`,
`ContentInfo` (with `derive`, ≈120 lines; without, ≈250 lines of manual `Encode`
impls). Fallback: the pure in-house writer (§ above) if the project would rather
not expose a `der` type in any `pub` signature — note `der` need not appear in
pdfcer-core's *public* API either way.

---

## 6. (E) `sha2` / `digest` generation check

`rsa 0.10.0-rc.18` → `digest 0.11`, `sha2 0.11` (`Cargo.toml`
`[dependencies.sha2] version = "0.11" features=["oid"]`), `signature 3.0.0-rc.10`
(resolves to 3.0.0), `pkcs8 0.11`, `rand_core 0.10`. `p256`/`p384 0.14`, `ecdsa 0.17`,
`hmac 0.13`, `pbkdf2 0.13`, `pkcs5 0.8.1`, `pkcs12 0.2.0-pre.0` → `digest 0.11` /
`sha2 0.11`. **Full-stack lock: single `sha2 0.11.0`, single `digest 0.11.3`,
`cargo tree -d` empty.** Against pdfcer's real `Cargo.lock` the stack's shared
crates already match: `sha2 0.11.0, cbc 0.2.1, cipher 0.5.2, crypto-common 0.2.2,
digest 0.11.3, hybrid-array 0.4.14, inout 0.2.2, subtle 2.6.1, zeroize 1.9.0,
typenum 1.20.1, syn 2.0.119, proc-macro2, quote, unicode-ident, num-traits, cfg-if,
cpubits` — identical versions; `aes 0.9.2 → 0.9.3` and `cpufeatures 0.3.0 → 0.3.1`
are semver-compatible lock bumps. **No new duplicate is created.** (Pre-existing
duplicates `sha2 0.10.9`/`digest 0.10.7`/`block-buffer 0.10.4`/`cpufeatures 0.2.17`
belong to `pdfcer-fetch`, not to this stack.) Do not take `p12 0.6.3` — it alone
would reintroduce the 0.10 generation.

---

## 7. Measured stacks side by side

| Stack (scratch member) | Unique crates | Of which proc-macro | New to pdfcer's `Cargo.lock` | `unsafe` carriers | wasm32 | Pre-release pins |
|---|---|---|---|---|---|---|
| RSA only (`m_rsa`) | 25 | 0 | — | 12 | OK | `rsa rc.18`, `pkcs1 rc.4` |
| ECDSA only (`m_ecdsa`) | 34 | 0 | — | 13 | OK | none |
| PKCS#12 RustCrypto (`m_pkcs12`) | 47 | 5 | — | 19 | OK | `pkcs12 pre.0`, `cms pre.1` |
| CMS types-only (`m_cms`) | 11 | 5 | — | 6 | OK | `cms pre.1` |
| **Full RustCrypto, no `cms/builder`** (`m_full`) | **66** | 5 | **46** | **24** | OK | `rsa`, `pkcs1`, `pkcs12`, `cms` |
| Full RustCrypto **with** `cms/builder` | 102 lines / ≈70 unique | — | — | — | **FAILS (11 errors, also native)** | + `aes-kw`, `ansi-x963-kdf rc.2`, `sha3`, `keccak`, `hkdf` |
| **LEAN** (`m_lean`): `rsa[sha2,encoding]` + `p256/p384[ecdsa,pkcs8]` + `signature` + `sha2` + `sha1` + `hmac` + `pbkdf2[hmac]` + `des` + `rc2` + `cbc[alloc,block-padding]` + `aes` + `der[alloc,oid]` | **48** | 0 | **32** | **18** | OK | `rsa rc.18`, `pkcs1 rc.4` |
| LEAN + `der/derive` (`m_lean_derive`) | 53 | 5 | 33 (`der_derive`) | 21 (the 3 proc-macro helpers) | OK | same |
| LEAN + `cms` types (`m_lean_cms`) | 56 | 5 | 36 (+`cms`, `flagset`, `x509-cert`) | 22 | OK | + `cms pre.1` |
| `p12-keystore` (`m_p12ks`) | 78 | 12 | — | — | **FAILS** (`getrandom 0.4` needs `wasm_js`) | `pkcs12 pre.0`, `cms pre.1` |
| `p12` (`m_p12`) | 20 | 0 | — | — | **FAILS** (`getrandom 0.2` needs `js`) | none (stale 2022) |

`m_lean` full list (48): `aes 0.9.3, base16ct 1.0.0, base64ct 1.8.3, block-buffer
0.12.1, block-padding 0.4.2, cbc 0.2.1, cfg-if 1.0.4, cipher 0.5.2, cmov 0.5.4,
const-oid 0.10.2, cpubits 0.1.1, cpufeatures 0.3.1, crypto-bigint 0.7.5,
crypto-common 0.2.2, crypto-primes 0.7.2, ctutils 0.4.2, der 0.8.2, des 0.9.0,
digest 0.11.3, ecdsa 0.17.0, elliptic-curve 0.14.1, ff 0.14.0, group 0.14.0, hmac
0.13.0, hybrid-array 0.4.14, inout 0.2.2, num-traits 0.2.19, p256 0.14.0, p384
0.14.0, pbkdf2 0.13.0, pem-rfc7468 1.0.0, pkcs1 0.8.0-rc.4, pkcs8 0.11.0, primefield
0.14.0, primeorder 0.14.0, rand_core 0.10.1, rc2 0.9.0, rfc6979 0.6.0, rsa
0.10.0-rc.18, sec1 0.8.1, sha1 0.11.0, sha2 0.11.0, signature 3.0.0, spki 0.8.0,
subtle 2.6.1, typenum 1.20.1, wnaf 0.14.1, zeroize 1.9.0`.

`m_lean` **new to pdfcer's lock (32):** `base16ct, base64ct, block-padding, cmov,
const-oid, crypto-bigint, crypto-primes, ctutils, der, des, ecdsa, elliptic-curve,
ff, group, hmac, p256, p384, pbkdf2, pem-rfc7468, pkcs1, pkcs8, primefield,
primeorder, rand_core, rc2, rfc6979, rsa, sec1, sha1, signature, spki, wnaf`.

`m_lean` **`unsafe` carriers (18) with line counts:** `aes 60` (already accepted,
decision 039), `sha2 53` (accepted), `hybrid-array 37` (already in lock), `inout
30` (in lock), `block-buffer 21` (in lock), `zeroize 17` (in lock), `cmov 25 (3
asm!)`, `crypto-bigint 14`, `cpufeatures 11 (1 asm)`, `sha1 7 (1 asm)`, `base16ct 4`,
`base64ct 4`, `der 4`, `elliptic-curve 4`, `subtle 2` (in lock), `const-oid 1`,
`num-traits 1` (in lock), `pem-rfc7468 1`. **Genuinely new `unsafe` carriers vs
today's lock: 11** (`cmov`, `crypto-bigint`, `cpufeatures 0.3.1`†, `sha1`,
`base16ct`, `base64ct`, `der`, `elliptic-curve`, `const-oid`, `pem-rfc7468`, plus
`aes 0.9.3` as a patch bump). †`cpufeatures 0.3.0` is already present. All of
`rsa`, `p256`, `ecdsa`, `pkcs8`, `spki`, `signature`, `pkcs1`, `sec1`, `rfc6979`,
`primefield`, `ff`, `wnaf`, `ctutils`, `des`, `rc2`, `cbc`, `hmac`, `pbkdf2`,
`crypto-primes`, `rand_core`, `digest`, `cipher`, `crypto-common` are **0-`unsafe`**
(many `#![forbid(unsafe_code)]`).

---

## 8. Recommended stack

**Take (LEAN, `m_lean` shape) — 48 crates resolved, 32 new to the lock, 18
`unsafe` carriers of which 11 are new, wasm32-clean, one pre-release pin
(`rsa`).**

```toml
# pdfcer-core/Cargo.toml — SIGNING (private-key side; decision 039's condition: a
# secret is handled, constant time is required, in-crate verify code is barred).
rsa       = { version = "0.10.0-rc.18", default-features = false, features = ["sha2", "encoding"] }
p256      = { version = "0.14",         default-features = false, features = ["ecdsa", "pkcs8"] }
p384      = { version = "0.14",         default-features = false, features = ["ecdsa", "pkcs8"] }
signature = { version = "3.0",          default-features = false }
der       = { version = "0.8.1",        default-features = false, features = ["alloc", "oid"] }  # + "derive" if hand-declared CMS structs are preferred (+5 build-time crates)
# PKCS#12 import
sha1      = { version = "0.11", default-features = false }   # Digest impl for PKCS#12 KDF + legacy MAC
hmac      = { version = "0.13", default-features = false }
pbkdf2    = { version = "0.13", default-features = false, features = ["hmac"] }
des       = { version = "0.9",  default-features = false }
rc2       = { version = "0.9",  default-features = false }
# aes 0.9, cbc 0.2.1, sha2 0.11 already present; cbc needs features = ["alloc", "block-padding"] for PKCS#7 unpadding
```

1. **RSA:** `rsa 0.10.0-rc.18` `[sha2, encoding]`, **no `getrandom`**; sign via
   `RandomizedPrehashSigner::sign_prehash_with_rng` (PKCS#1 v1.5 → blinded; PSS →
   salted + blinded) with a `rand_core 0.10::TryCryptoRng` adapter over
   `crypto::rng::fill`. Refuses on wasm32 with `RngError::Unavailable` (rule 4:
   print/report why).
2. **ECDSA:** `p256`/`p384 0.14` `[ecdsa, pkcs8]` — RFC 6979 deterministic
   `PrehashSigner`; `Signature::to_der()` into CMS.
3. **PKCS#12:** in-house PFX walker on `asn1.rs` (+≈120 lines), in-house RFC 7292
   B.2 KDF (+≈80 lines, generic over `digest::Digest`), MAC via `hmac`, PBES2
   params via the walker + `pbkdf2` + `aes`/`des` + `cbc`, legacy PBES1 via `des`
   `TdesEde3` / `rc2::new_with_eff_key_len(k, 40)` + `cbc`. Reference impl:
   `p12-keystore 0.3.1/src/pbes1.rs`. Rule 4: disclose which scheme/iteration count
   the file used and whether the MAC verified.
4. **CMS/DER:** `der 0.8` (already transitive) + pdfcer-declared `SignedData` /
   `SignerInfo` / `ESSCertIDv2` structs; compute `messageDigest` over the PDF byte
   ranges; encode `signedAttrs` as `SetOfVec` (0x31, DER-sorted); sign; assemble
   detached `SignedData` (`eContent` absent); leave `unsignedAttrs` `[3]` for a
   later RFC 3161 token (Pass B-T). Round-trip through the existing `cms.rs` /
   `signature_verify.rs` as the in-crate oracle.
5. **Not taken:** `cms` (builder broken; types replaceable; pre-release),
   `pkcs12` (no decrypt/MAC; pins `cms =pre.1`), `pkcs5`/`pkcs8[encryption]`
   (PBES1 absent; 8 unused AEAD/scrypt crates), `x509-cert` (verifier already parses
   certs in-house; certs pass through raw), `p12-keystore` / `p12` (wasm32 fail,
   duplicate parsers / stale generation), `ring` (unchanged, `PRIOR_ART.md`).

Rule-13 classification of every crate above: **permissive** (MIT OR Apache-2.0 /
Apache-2.0 OR MIT); regenerate `THIRD_PARTY_LICENSES.md` with `cargo-about` when
they enter. GUI-core invariant: none of them has a windowing or network dependency
(`cargo tree` lists above are exhaustive).

---

## 9. Risks, ranked

1. **`rsa` is a release candidate under an open, unpatched advisory
   (RUSTSEC-2023-0071).** `cargo audit`/`cargo deny` in CI will flag it forever until
   upstream marks a patched version — and upstream has said nothing about doing so
   (#19 closed; #626 open; #680 unmerged). Mitigation: document the signing-vs-
   decryption distinction (§2) in the module header, use the blinded `Randomized*`
   path only, add an explicit `ignore = ["RUSTSEC-2023-0071"]` with that rationale
   if an audit gate is ever added, and re-check on each `rsa` bump. Residual: rc→final
   API churn (rc.11→rc.18 in four months, `pkcs8 0.11` migration in rc.18).
2. **PKCS#12 legacy decryption + MAC are hand-written crypto plumbing either
   way** (no crate does it on the current RustCrypto generation, wasm32-clean). Bugs
   here are *availability* bugs (file won't open), not confidentiality bugs, but the
   KDF's BMPString+NUL and RC2-40 effective-key details are classic mis-
   implementations. Mitigation: fixtures generated by both `openssl pkcs12 -export
   -legacy` and OpenSSL 3 default (rule 7: synthetic, self-signed), plus
   `p12-keystore`'s test vectors as a cross-check.
3. **DER writer correctness (X.690 §11.6 `SET OF` ordering, minimal INTEGER, ESS
   `signing-certificate-v2`)** is judged by strict third-party verifiers (Adobe
   Reader, ETSI conformance checkers), not by pdfcer's own lenient-by-design
   parser. Mitigation: use `der`'s `SetOfVec` for the attribute set rather than a
   hand sort; CI oracle with `openssl cms -verify` + Acrobat Reader spot-check
   (available per memory note); ETSI EN 319 122-1 clause citations in doc comments
   (dispatch `pdfcer-spec-librarian` for the PAdES B-B attribute list before
   coding).

Secondary: `der 0.8.x` is being patched monthly (0.8.0 yanked today) — pin
`>=0.8.1`; 11 new `unsafe`-carrying crates enter `pdfcer-core`'s tree (the
decision-039 shape — constant-time backends, `cmov`'s 3 `asm!` files — accepted
here because a private key is handled); `ecdsa`/`p256 0.14` are two months old
(Jul 2026) with 2 M downloads vs 0.13's 107 M — young but final.
