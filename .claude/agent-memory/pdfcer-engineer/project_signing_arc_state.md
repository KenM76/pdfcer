---
name: signing-arc-state
description: 2026-09-05 snapshot of the digital-signing arc — what shipped (PAdES B-B from .pfx, Passes 10.7–10.9), the crate decision (137), what is deliberately NOT done (B-T/LT, Windows store, composed appearance), and the two API traps hit
metadata:
  type: project
---

# Signing arc — state as of 2026-09-05

**Shipped in one commit (`7734261`): Passes 10.7 (PKCS#12 + `Signer` trait), 10.8
(CMS build, in-house DER writer), 10.9 (`EditSession::sign` + `pdfcer sign`).**
PAdES B-B / `adbe.pkcs7.detached`, RSA v1.5 + PSS, ECDSA P-256/P-384, incremental
only, self-verified with `signature_verify` before returning, cross-checked in
tests by `openssl cms -verify -noverify`. A second signature appends and the first
still verifies (tested).

**Why it is shaped this way:** the operator approved (decision 136) ".pfx first,
then Windows store / PKCS#11 as SHELL-side `Signer` impls" — the key never enters
the engine; every source is hash-in/signature-out. Crate stack is decision 137
(survey in `docs/signing-crate-survey.md`): `rsa 0.10 rc` under the OPEN Marvin
advisory, accepted because the residual channel is a DECRYPTION oracle signing
never runs, and only the blinded `Randomized*` API is used.

**Deliberately not done (say so if asked, do not re-derive):**
- Visible signatures draw a thin frame only — no text appearance.
- Level is always `B-B`; B-T needs a shell TSA round trip (10.11), B-LT/LTA need
  revocation (10.6). `report.pades_level` never claims higher.
- Encrypted documents are refused outright (the incremental writer cannot append
  to an encrypted base) — broader than Acrobat's permission-bit gate.
- No certifying (`/DocMDP`) signatures authored yet; approval only.
- PKCS#12 with BER indefinite lengths, RC4 bags, PK-integrity mode: refused by name.
- RSA signing refuses on wasm32 (no RNG for blinding); ECDSA (RFC 6979) works there.

**Two API traps that cost compile cycles (rsa rc.18 / signature 3.0):**
- `try_sign_digest_with_rng` takes a CLOSURE `Fn(&mut D) -> Result<()>` that feeds
  the digest, not a digest instance.
- pkcs1v15 `SigningKey` has NO `RandomizedPrehashSigner`; blinded signing is only
  reachable through `RandomizedDigestSigner` — hence `Signer::sign` takes the
  MESSAGE (signedAttrs DER), and the signer hashes.

**How to apply:** the next signing increments are 10.10 (shell-side signers —
`sign::verify_raw_signature` exists so a shell can prove its custodian plumbing)
and 10.11 (B-T: `unsignedAttrs` token; reserve must grow). Test fixtures live in
`fixtures/synthetic/signing/` (password `pdfcer`; regenerate with
`tools/gen-signing-fixtures.py` — keys are random, tests assert round trips only).
