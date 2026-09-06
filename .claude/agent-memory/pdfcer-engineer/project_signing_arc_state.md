---
name: signing-arc-state
description: 2026-09-06 snapshot of the digital-signing arc — shipped (10.7–10.9 PAdES B-B, 10.14 hardening + composed appearance, 10.12 certifying/DocMDP, 10.13 sign into a pre-placed field with /Lock→/FieldMDP and total /SV enforcement), the crate decision (137), what is deliberately NOT done, the measured as-received CMS rule, and the API traps
metadata:
  type: project
---

# Signing arc — state as of 2026-09-06

**Shipped:** 10.7–10.9 (`7734261`, v0.40.0): PKCS#12 + `Signer` trait, in-house
DER CMS builder, `EditSession::sign` + `pdfcer sign`, PAdES B-B /
`adbe.pkcs7.detached`, RSA v1.5+PSS, ECDSA P-256/P-384, incremental only,
self-verified. Then in the 0.42.0 batch (unreleased when written):
- **10.14 (`187fa09`)** — CMS sabotage tests via the doc-hidden
  `cms_build::build_with(.., Sabotage)`; no-`localKeyId` PFX (pairing by public
  key); pyHanko foreign-signature fixtures BOTH directions (pyHanko is installed
  and works: `tools/gen-foreign-signature-fixtures.py`); P-384 store; the visible
  appearance is now composed text (signer CN / Date / Reason / Location) in inline
  Helvetica, shrink-to-fit 10→4 pt, `AppearanceOverflow` refusal, lines on
  `SignReport::appearance_lines`.
- **10.12 (`02bb1ba`)** — `SignRequest::certify: Option<MdpPermission>` writes
  `/Reference [SigRef /DocMDP]` + catalog `/Perms`; one per document, FIRST
  signature only (`AlreadyCertified`, `CertificationNotFirst`);
  `SignatureVerdict::certification: Option<u8>`; CLI `--certify --mdp-level`.
- **10.13 (`ab40127`)** — `field_name` of an existing EMPTY merged `/FT /Sig`
  field signs INTO it; `/Lock` → `/FieldMDP` (copied Action/Fields, `/Data` =
  catalog); `/SV` enforced totally by `apply::check_seed_value` (required-unmet →
  `SeedValueViolated`; unevaluable → `SeedValueUnevaluable`; recommended-unmet →
  `SignReport::notes`). Fixtures `tools/gen-sig-field-fixtures.py`.

**Why it is shaped this way:** decision 136 (".pfx first, then Windows store /
PKCS#11 as SHELL-side `Signer` impls" — the key never enters the engine) and
decision 137 (`rsa 0.10 rc` under the open Marvin advisory, accepted because
signing never runs the decryption oracle; blinded API only).

**MEASURED, not assumed (2026-09-06):** OpenSSL 1.1.1s and pdfcer both hash
signedAttrs AS RECEIVED (retagged 0x31); an unsorted-but-consistently-signed SET
verifies in both; reordering after signing breaks both. The spec RAG's CB-4 was
split into CB-4a–d over this. Keep the as-received rule — a strict re-sorting
verifier would refuse what other readers accept.

**Deliberately not done (say so if asked, do not re-derive):**
- Level is always `B-B`; B-T (10.11) needs a TSA round trip — the seed-value
  evaluator refuses a REQUIRED `/TimeStamp` by name; B-LT/LTA need revocation.
- `/SV /Cert` (Table 235) is refused by name, not evaluated; non-merged (`/Kids`)
  signature fields are refused (`FieldHasKids`).
- Encrypted documents refused outright; RSA refuses on wasm32.
- No verb AUTHORS an empty signature field yet (a forms verb, when asked).

**API traps that cost compile cycles:**
- `try_sign_digest_with_rng` takes a CLOSURE feeding the digest; pkcs1v15
  `SigningKey` has no `RandomizedPrehashSigner` → `Signer::sign` takes the MESSAGE.
- The sign verb's helpers live in a DIFFERENT `impl` block from the verb: fully
  qualify `crate::sign::apply::…` there and gate with `#[cfg(feature = "signing")]`.
- `signature_verify` and `signature::census` must read `/P` the same way
  (default 2); both now do — keep them in step.

**How to apply:** next increments are 10.10/10.11 (shell signers, B-T),
`/SV /Cert`, `/Kids` fields, an author-empty-field forms verb. Test fixtures:
`fixtures/synthetic/signing/` (password `pdfcer`; `gen-signing-fixtures.py` adds
shapes from the existing rsa2048 material by default, `--regen` re-mints all).
