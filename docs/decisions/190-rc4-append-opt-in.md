# Decision 190 — Keeping an existing RC4 handler on append is opt-in

- **Date:** 2026-10-04
- **Status:** DECIDED; implementation in `Pass 471.0`.
- **Authored by:** `autonomous-builder` / KenAgent.
- **Narrows:** standing rule **W14** ("pdfcer never writes RC4") to "pdfcer
  never *chooses* RC4; keeping an existing RC4 handler on an incremental save
  is opt-in, default Refuse". Closes the W14 operator question.
- **Clauses:** ISO 32000-2 §7.6.2 (RC4 deprecated; per-object key from the
  object number and generation), §7.6.3 (`/Encrypt`, `/ID[0]` carried on
  append), §7.5.6 (incremental update).

## 1. Decision

- `writer::Rc4Append {Refuse (default), Preserve}`, set per document with
  `Document::set_rc4_append` / `EditSession::set_rc4_append`. CLI: the global
  flag `--allow-rc4-append`.
- Covers every RC4 form: `/V 1`, `/V 2`, and `/V 4` with `/CFM /V2`.
- Preserve appends under the document's own RC4 key. Existing signatures stay
  valid.
- Every Preserve save is disclosed: `SaveReport::rc4_keystream_reused =
  Some(n)` (also on `SignReport`, `DocTimestampReport`), `n` counted in the
  writer. The CLI prints it on stderr every time (rule 4).
- The refusal message names the flag. Permission gating (`/P`) is unchanged.
- New encryption never uses RC4.

## 2. Why

- Re-encrypting to AES-256 invalidates every existing signature; refusing
  outright leaves an RC4 signed form uneditable. The opt-in is the only route
  that keeps both the signature and the edit.
- RC4's per-object key depends only on the object number and generation, so an
  edited object rewritten under its old id is encrypted with the same keystream
  as its previous revision, which stays in the file (§7.5.6). XOR of the two
  ciphertexts cancels the keystream. That is why it is opt-in and why `n` is
  reported: verbatim re-emissions (copied ciphertext), object-stream members
  (never encrypted under their own key) and created objects (fresh numbers)
  do not count.

## 3. Deviation from the consultant's sketch

The ruling put the knob on `SaveOptions`. It lives on the `Document` instead:
the edit-time gate (`encryption_gate::forbids`, every mutating verb) refuses
through `DocumentEncryption::appendable()` before a save is ever attempted, so
a save option could not un-gate the edit. One source of truth — the document's
encryption state — answers both the edit gate and the writer.
