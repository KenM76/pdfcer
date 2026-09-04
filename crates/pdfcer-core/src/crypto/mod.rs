//! Document encryption — ISO 32000-1 §7.6.
//!
//! # What this module is for
//!
//! A PDF may be encrypted, and until this module existed pdfcer refused every
//! such file with [`XrefErrorKind::EncryptionUnsupported`]. That refusal was
//! honest and is still the fallback, but it covered the *whole* of §7.6
//! including the commonest case in the wild: a document with an **empty user
//! password** and a non-empty owner password, which every other viewer opens
//! silently and without a prompt. To an operator, pdfcer refusing a file Chrome
//! opens does not read as a scoped capability gap.
//!
//! # Increments 1, 2 and 3 — read direction only
//!
//! | | Status |
//! |---|---|
//! | `/V` 1, 2 at `/R` 2, 3 (RC4, 40–128 bit) | **implemented** |
//! | `/V` 4 at `/R` 4 with `/CFM /V2` (RC4 via crypt filter) | **implemented** |
//! | `/V` 4 with `/CFM /AESV2` (AES-128) | **implemented** (increment 2) |
//! | `/V` 5, `/R` 5 with `/CFM /AESV3` (AES-256) | **implemented** (increment 3) |
//! | `/V` 5, `/R` 6 (AES-256 hardened) | **read AND write** (`Pass 5.4`) — `/R` 5's harness with Algorithm 2.B substituted for SHA-256 ([`r5::Hasher`]); 2.B sourced from ISO 32000-2:2020 (2026-08-12) |
//! | Public-key handler, third-party handlers | refused by handler name |
//! | **Writing** encrypted documents | **AES-256 `/R` 6 only** (`Pass 5.4`, [`encrypt`]). RC4 and `/R` 2–5 are never written: W14, and ISO 32000-2 §7.6.4.1 deprecates handler revisions 1–5, leaving `/R` 6 the only non-deprecated AES-256 revision (**W17**) |
//!
//! **Do not read "AES-256 is implemented" as "AES-256 is done".** `/R` 6 is
//! the default for everything Acrobat X and later produced with the "AES-256"
//! setting, and it is likely the *common* AES-256 case in the wild. Increment
//! 3 covers the 2008–2011 `/R` 5 population and nothing after it.
//!
//! The refusals are deliberately distinguishable. "pdfcer hasn't implemented
//! AES yet", "no reader on earth may open this file", and "the algorithm isn't
//! published anywhere we can source it" are three different facts with three
//! different next actions, and collapsing them into one message throws away
//! the only part an operator can act on.
//!
//! # Why RC4 came first, and why that is not a security statement
//!
//! Increment order was chosen on **dependency risk**, not on cipher strength.
//! `pdfcer-core` had no cryptographic dependency at all before this; RC4 and
//! MD5 are frozen, tiny, and needed only to *read* documents other producers
//! already made, so implementing them in-crate avoided a rule-13 dependency
//! decision entirely. AES does not qualify for that reasoning and took a
//! dependency in increment 2 — see [`md5`]'s module docs, which state the
//! limits of the judgement in full, and [`aes`]'s, which honour them.
//!
//! RC4 is broken. So is MD5. So, structurally, is PDF encryption at `/V` 1–4:
//! there is **no integrity protection anywhere in it** (negative result N7 —
//! no MAC, and `/P` sits in the file as an editable plaintext integer). pdfcer
//! reading these files is a compatibility obligation. Nothing here should be
//! read as a recommendation to produce them.
//!
//! # Permissions are reported, never silently enforced
//!
//! §7.6.3.1 says it outright: *"There is nothing inherent in PDF encryption
//! that enforces the document permissions."* Readers "shall respect the intent
//! of the document creator". So [`standard::Permissions`] is a **report** of
//! what the author asked for, and any place pdfcer acts on it must disclose
//! that it is doing so (project rule 4). ISO 32000-1 specifies no mapping from
//! a permission bit to a reader operation at all (**N4**) — "assemble the
//! document" is not an object-level predicate — so the mapping is pdfcer's own
//! product decision and has to be visible as such.
//!
//! # Layout
//!
//! - [`md5`] — RFC 1321 digest. Key derivation only.
//! - [`rc4`] — the stream cipher. Encryption and decryption are one operation.
//! - [`aes`] — AES-128 and AES-256 for `/AESV2` and `/AESV3`, in the **three**
//!   modes `/R` 5 uses (T25). Together with [`r5`]'s SHA-256, the one place
//!   `pdfcer-core` takes a cryptographic dependency, and the one place its
//!   dependency tree is not compiler-enforced free of `unsafe` (decision 039,
//!   which `sha2` extends: `sha2` selects its backends on a `sha2_backend`
//!   cfg exactly as `aes` does on `aes_backend`, so the same reasoning and the
//!   same bounded exception apply).
//! - [`r5`] — Algorithms 3.2a and 3.8–3.13: the `/R` 5 password preparation,
//!   SHA-256 authentication, `/UE`/`/OE` key unwrap, and the `/Perms` check.
//! - [`standard`] — the `/Standard` handler: `/Encrypt` parsing, Algorithms
//!   1–7, authentication, per-object keys, and the dispatch between the two
//!   key derivations.
//!
//! [`XrefErrorKind::EncryptionUnsupported`]: crate::xref::XrefErrorKind::EncryptionUnsupported

pub mod aes;
pub mod apply;
pub mod bignum;
pub mod ecdsa;
pub mod encrypt;
pub mod md5;
pub mod r5;
pub mod r6;
pub mod rc4;
pub mod rng;
pub mod rsa;
pub mod sha1;
pub mod standard;

pub use r5::{PermsCheck, PreparedPassword};
pub use standard::{
    Aes256Keys, AuthKind, Cipher, EncryptionConfig, EncryptionUnsupported, FileKey, PermissionBit,
    Permissions,
};
