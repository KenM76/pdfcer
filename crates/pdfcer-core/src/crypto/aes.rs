//! AES for PDF — `/AESV2` (AES-128-CBC, ISO 32000-1 §7.6.2) and `/AESV3`
//! (AES-256, Adobe ExtensionLevel 3 §3.5).
//!
//! # Why this is a dependency when [`md5`](super::md5) and [`rc4`](super::rc4)
//! are not
//!
//! [`md5`](super::md5)'s module docs record the judgement that MD5 and RC4 were
//! cheaper and lower-risk to write in-crate than to depend on, **and state in
//! the same breath that the reasoning does not extend to AES**: AES has real
//! implementation hazards (timing, key schedules, mode handling), a live
//! ecosystem, and well-audited permissive crates. That sentence was written
//! before this module existed, precisely so it could not be quietly reused to
//! justify hand-rolling the next cipher. It is honoured here.
//!
//! So the block cipher and the CBC chaining come from RustCrypto's `aes` and
//! `cbc`. What stays in-crate is the part that is **not** a cryptographic
//! hazard and **is** a policy decision: which bytes are the IV, and what to do
//! about padding that does not verify. See "Padding is a policy, not a
//! primitive" below.
//!
//! # The wire format (§7.6.2, and TRAP T5)
//!
//! ```text
//! ciphertext = IV(16 bytes, random) ‖ AES-128-CBC(object_key, IV, pad(plaintext))
//! pad        = PKCS#5/7 to a 16-byte block, ALWAYS present
//!              (a full 16-byte block of 0x10 when the plaintext is already
//!               a multiple of 16 — there is no "no padding needed" case)
//! ```
//!
//! Three consequences that are easy to get wrong and silent when you do:
//!
//! - **The IV is prefixed to the data, not stored in the dictionary.** There is
//!   no `/IV` key anywhere in an encryption dictionary; a reader that looks for
//!   one finds nothing and decrypts with a zero IV, which corrupts exactly the
//!   *first* 16 bytes and leaves the rest perfect. On a `/FlateDecode` stream
//!   that is a header failure; on a raw stream it is 16 bytes of noise most
//!   viewers never draw.
//! - **T5 — `/Length` on an AES stream counts the IV and the padding.** It is
//!   the length of the *ciphertext*, so the plaintext is always strictly
//!   shorter, by at least 17 bytes (16 IV + at least 1 pad byte). See
//!   [`crate::document`]'s decryption walk for what that costs the object model.
//! - **T1 — the per-object key derivation gains four `sAlT` bytes** in its MD5
//!   *input*, but the derived key stays `min(n+5, 16)` bytes. That is handled in
//!   [`FileKey::object_key`](super::standard::FileKey::object_key), not here;
//!   this module is handed a finished 16-byte key.
//!
//! # Padding is a policy, not a primitive
//!
//! `cbc` can strip PKCS#7 itself (`decrypt_padded_b2b::<Pkcs7>`) and returns an
//! `Err` when the padding does not verify. This module deliberately does **not**
//! use that, and takes the raw block API instead, because "the padding did not
//! verify" is a question about a possibly-damaged file, and the right answer to
//! it is a pdfcer product decision rather than a cryptographic one:
//!
//! **When the padding verifies, it is stripped. When it does not, every
//! decrypted byte is returned instead, and nothing is reported.**
//!
//! The reasoning, in order:
//!
//! 1. **The key is already known to be right.** Padding is checked *after*
//!    authentication has succeeded against `/U` or `/O`. So invalid padding
//!    here does not mean "wrong password" — it means the bytes are damaged, or
//!    the producer wrote non-conforming padding.
//! 2. **Keeping the bytes is strictly better than discarding them.** The great
//!    majority of PDF streams are `/FlateDecode`, which is self-terminating —
//!    it stops at its own end-of-stream marker and simply ignores up to 16
//!    trailing junk bytes. Returning the unstripped plaintext therefore
//!    recovers the stream completely in the common case, where returning an
//!    error or the untouched ciphertext recovers nothing at all.
//! 3. **The usual argument against lenient padding does not apply.** Lenient
//!    PKCS#7 handling is dangerous when an attacker can observe whether it
//!    succeeded — the padding-oracle attack. pdfcer is a local file reader: there
//!    is no oracle, no attacker-observable response, and no adaptive query. The
//!    hazard being traded away does not exist in this program, and the recovery
//!    being bought is real.
//!
//! This is a deliberate leniency and it is tested in both directions
//! (`invalid_padding_keeps_every_byte`, `valid_padding_is_stripped`) so it
//! cannot decay into an accident.
//!
//! # ★ AES-256 uses THREE DIFFERENT MODES, and mixing them is silent (TRAP T25)
//!
//! At `/V` 5 the same 32-byte file encryption key is fed to AES in three
//! different configurations, in three different places, within one document
//! load. They look interchangeable and are not:
//!
//! | Operation | Function here | Mode | IV | Padding |
//! |---|---|---|---|---|
//! | Document strings and streams (Algorithm 3.1a) | [`decrypt_cbc_256`] | CBC | **random, prefixed to the data** | PKCS#7, always present |
//! | Unwrapping `/UE` or `/OE` (Algorithms 3.2a/3.8/3.9) | [`unwrap_key_cbc_256`] | CBC | **all zero** | **none** |
//! | Decrypting `/Perms` (Algorithms 3.2a/3.13) | [`decrypt_ecb_256_block`] | **ECB** | **none — ECB has no IV** | **none** |
//!
//! Every wrong pairing produces plausible-looking bytes rather than an error.
//! Unwrap `/UE` with the CBC routine that expects a prefixed IV and you consume
//! the first 16 bytes of the wrapped key as an IV, yielding a 16-byte "key";
//! unwrap it with a *non-zero* IV and you get a 32-byte key that is wrong in
//! exactly its first 16 bytes, which then decrypts every object in the document
//! to noise with no diagnostic anywhere near the cause.
//!
//! **`/Perms` takes no IV at all.** Adobe's ExtensionLevel 3 text says "ECB
//! mode with an initialization vector of zero", and ISO 32000-2's public
//! errata **strike** that phrase from all three places it appears (Algorithm
//! 2.A bullet (f), Algorithm 10 bullet (f), Algorithm 13 bullet (a)). ECB
//! chains nothing, so no behaviour follows from the correction — its value is
//! that [`decrypt_ecb_256_block`] has no IV parameter to pass the wrong thing
//! to. Source: `iso32000__delta__pdf20_encryption.md` § D6.
//!
//! # What is NOT here
//!
//! **The key derivation.** This module is handed finished keys. Algorithm 1
//! (per-object keys, `/V` 1–4) lives in
//! [`FileKey::object_key`](super::standard::FileKey::object_key); Algorithms
//! 3.2a and 3.11–3.13 (the `/R` 5 password and key algorithms) live in
//! [`r5`](super::r5). The split is deliberate: derivation is where clause 7.6's
//! traps are, and it is testable against published vectors without a cipher in
//! the way.
//!
//! **Encryption.** pdfcer cannot write an encrypted document at all — both save
//! paths refuse one (`WriteError::EncryptedSaveUnsupported`). Adding an encrypt
//! function here before there is a writer that could call it would be an
//! untested code path wearing the appearance of a capability.
//!
//! **`/R` 6.** `/R` 6 is *not* a different cipher — it is `/R` 5's harness with
//! Algorithm 2.B substituted for SHA-256, and 2.B is unsourced past step (a).
//! So nothing in this module changes for it, and nothing here unblocks it.

use aes::cipher::{Block, BlockCipherDecrypt, BlockModeDecrypt, KeyInit, KeyIvInit};
use aes::{Aes128, Aes256};

/// One AES block, as the cipher crates model it.
///
/// Written in terms of [`Aes128`] but shared by both key lengths on purpose:
/// AES's block size is 16 bytes for AES-128, AES-192 and AES-256 alike, so
/// `Block<Aes128>` and `Block<Aes256>` are the *same* concrete type. That is
/// what lets [`blocks_of`] and the framing helpers serve every routine below
/// instead of being written twice, and it is a property of the algorithm
/// rather than a coincidence of this crate's generics.
type AesBlock = Block<Aes128>;

/// The AES block size in bytes. Fixed at 16 for every AES key length — it is
/// the *key* that varies between AES-128/192/256, never the block.
pub const BLOCK_LEN: usize = 16;

/// The length of the initialisation vector prefixed to every `/AESV2`
/// ciphertext. Equal to the block size, which is a property of CBC rather than
/// a coincidence: CBC XORs the IV into the first block.
pub const IV_LEN: usize = 16;

/// The shortest byte string that can be a well-formed `/AESV2` ciphertext:
/// a 16-byte IV plus one full padded block. There is no shorter valid case,
/// because §7.6.2's padding is *always* present — an empty plaintext still
/// encrypts to a full block of `0x10`.
pub const MIN_CIPHERTEXT_LEN: usize = IV_LEN + BLOCK_LEN;

/// The length of an AES-256 key. Named rather than spelled `32` at every use
/// because `32` is also the length of `/UE`, of `/OE`, and of the file
/// encryption key itself — four different 32s within one algorithm.
pub const KEY_LEN_256: usize = 32;

type Aes128CbcDec = cbc::Decryptor<Aes128>;
type Aes256CbcDec = cbc::Decryptor<Aes256>;

/// Split `data` into its prefixed IV and a whole number of ciphertext blocks.
///
/// Shared by [`decrypt_cbc_128`] and [`decrypt_cbc_256`] because the framing is
/// a property of §7.6.2's *wire format*, not of the key length: both `/AESV2`
/// and `/AESV3` put a 16-byte random IV in front of the data (Table 3.22's
/// `AESV3` row repeats Table 25's `AESV2` wording verbatim on this point).
///
/// Returns `None` — meaning "no plaintext is recoverable" — when `data` cannot
/// hold an IV and at least one block, or when nothing but a partial block
/// follows the IV. A trailing partial block is dropped rather than treated as
/// fatal: CBC has no meaning for one, and the whole blocks in front of it are
/// still recoverable. See [`decrypt_cbc_128`]'s "Malformed input" section for
/// why every malformation here degrades instead of erroring.
fn split_iv_and_whole_blocks(data: &[u8]) -> Option<([u8; IV_LEN], &[u8])> {
    if data.len() < MIN_CIPHERTEXT_LEN {
        return None;
    }
    let (iv, body) = data.split_at(IV_LEN);
    let iv: [u8; IV_LEN] = iv.try_into().ok()?;

    let whole = body.len() - (body.len() % BLOCK_LEN);
    let body = body.get(..whole)?;
    if body.is_empty() {
        return None;
    }
    Some((iv, body))
}

/// Copy a whole-block byte slice into the `Block` values the cipher crates
/// take.
///
/// `body.len()` must already be a multiple of [`BLOCK_LEN`]; every caller
/// obtains it from [`split_iv_and_whole_blocks`] or checks it directly, so a
/// short final chunk is impossible and `chunks_exact` would silently drop one
/// if it were not.
fn blocks_of(body: &[u8]) -> Vec<AesBlock> {
    body.chunks_exact(BLOCK_LEN)
        .map(|c| {
            let mut b = AesBlock::default();
            b.copy_from_slice(c);
            b
        })
        .collect()
}

/// Flatten decrypted [`AesBlock`]s back into a byte vector.
///
/// Trivial, and named so the three routines below read as one sentence each
/// rather than as three copies of the same iterator chain.
fn flatten(blocks: Vec<AesBlock>) -> Vec<u8> {
    blocks.into_iter().flatten().collect()
}

/// Decrypt an `/AESV2` string or stream: strip the IV, run AES-128-CBC, and
/// remove PKCS#7 padding if it verifies.
///
/// `key` is the finished per-object key from Algorithm 1 — already salted with
/// `sAlT` (T1) by the caller. `data` is the raw ciphertext **including** its
/// leading IV, exactly as it sits in the file.
///
/// # Returns
///
/// The plaintext, which is always **shorter** than `data` by at least
/// [`MIN_CIPHERTEXT_LEN`] minus the last block's payload. Callers that track
/// byte spans must record the returned length rather than assuming the
/// length-preserving behaviour RC4 gave them.
///
/// # Malformed input
///
/// This returns a `Vec` rather than a `Result` because every failure mode here
/// is "this file is damaged", and the caller's only sensible response is the
/// same one it already has for a stream whose `/Length` overruns the buffer:
/// carry on and let the object fail to decode, with an error about the object.
/// Raising a distinct error per malformation would give the operator a
/// cryptographic message for a corruption problem.
///
/// - `data` shorter than [`MIN_CIPHERTEXT_LEN`], or a `key` that is not 16
///   bytes → an empty `Vec`. There is no plaintext to recover.
/// - A ciphertext body that is not a whole number of blocks → the trailing
///   partial block is **ignored**. CBC has no meaning for a partial block, and
///   the whole blocks before it are still recoverable.
/// - Padding that does not verify → every decrypted byte is returned unstripped.
///   See the module docs; this is deliberate.
///
/// # Examples
///
/// A round trip through the matching encryptor, showing that the IV travels
/// with the data and that the plaintext comes back shorter than the ciphertext:
///
/// ```
/// use pdfcer_core::crypto::aes::{decrypt_cbc_128, MIN_CIPHERTEXT_LEN};
///
/// // A ciphertext produced with key `[0x42; 16]` and IV `[0x24; 16]` over the
/// // plaintext `b"hello world! this is my plaintext."`.
/// let ciphertext: Vec<u8> = {
///     let mut v = vec![0x24; 16]; // the IV, as it appears in the file
///     v.extend_from_slice(&[
///         0xc7, 0xfe, 0x24, 0x7e, 0xf9, 0x7b, 0x21, 0xf0, 0x7c, 0xbd, 0xd2, 0x6c,
///         0xb5, 0xd3, 0x46, 0xbf, 0xd2, 0x78, 0x67, 0xcb, 0x00, 0xd9, 0x48, 0x67,
///         0x23, 0xe1, 0x59, 0x97, 0x8f, 0xb9, 0xa5, 0xf9, 0x14, 0xcf, 0xb2, 0x28,
///         0xa7, 0x10, 0xde, 0x41, 0x71, 0xe3, 0x96, 0xe7, 0xb6, 0xcf, 0x85, 0x9e,
///     ]);
///     v
/// };
///
/// let plain = decrypt_cbc_128(&[0x42; 16], &ciphertext);
/// assert_eq!(plain, b"hello world! this is my plaintext.");
/// assert!(plain.len() < ciphertext.len());
/// assert!(ciphertext.len() >= MIN_CIPHERTEXT_LEN);
/// ```
///
/// Anything too short to carry an IV and one block yields no plaintext:
///
/// ```
/// use pdfcer_core::crypto::aes::decrypt_cbc_128;
/// assert!(decrypt_cbc_128(&[0x42; 16], b"too short").is_empty());
/// ```
#[must_use]
pub fn decrypt_cbc_128(key: &[u8], data: &[u8]) -> Vec<u8> {
    // A key of the wrong length cannot be turned into an AES-128 key at all.
    // Algorithm 1 always yields 16 bytes for /AESV2 (`/Length` is 128, so
    // `min(16 + 5, 16)` is 16), so this is unreachable from the document path
    // -- but this is a `pub fn` and the check is one comparison.
    let Ok(key): Result<[u8; 16], _> = key.try_into() else {
        return Vec::new();
    };
    let Some((iv, body)) = split_iv_and_whole_blocks(data) else {
        return Vec::new();
    };

    let blocks = blocks_of(body);
    let mut out = vec![AesBlock::default(); blocks.len()];

    let mut dec = Aes128CbcDec::new(&key.into(), &iv.into());
    if dec.decrypt_blocks_b2b(&blocks, &mut out).is_err() {
        // `out` is allocated at exactly `blocks.len()`, so the only documented
        // error (output too small) cannot happen. Degrade rather than panic:
        // this crate parses untrusted input and must not abort its host.
        return Vec::new();
    }

    let mut plain = flatten(out);
    strip_pkcs7(&mut plain);
    plain
}

/// Decrypt an `/AESV3` string or stream: strip the IV, run AES-256-CBC, and
/// remove PKCS#7 padding if it verifies.
///
/// **Adobe ExtensionLevel 3, Algorithm 3.1a** (≡ ISO 32000-2 "1.A"). The wire
/// format is identical to `/AESV2`'s — Table 3.22's `AESV3` row repeats
/// "16-byte block size … random initialization vector as the first 16 bytes of
/// the stream or string" — so this is [`decrypt_cbc_128`] at a longer key, and
/// the padding policy, the malformed-input contract and the leniency argument
/// in the module docs all carry over unchanged.
///
/// **What does *not* carry over is the key.** `key` is the 32-byte **file
/// encryption key, used as-is** — TRAP **T24**. At `/V` 5 there is no
/// Algorithm 1, no object number, no generation number and no `sAlT`: the
/// supplement states it outright, "algorithm 3.1a uses the starting key
/// directly and does not modify the key at all", and adds "encrypt version 5
/// does not use MD5". Every string and every stream in the document shares one
/// key. Routing `/AESV3` through the Algorithm-1 code path — which is the
/// natural thing to do when adding a cipher to an existing handler — produces
/// garbage for every object in the file.
///
/// # Returns
///
/// The plaintext, shorter than `data` by at least 17 bytes (16 IV + at least
/// one pad byte). Callers tracking byte spans must record the returned length;
/// see [`decrypt_cbc_128`].
///
/// # Malformed input
///
/// Identical to [`decrypt_cbc_128`], with the key length check at 32 bytes
/// instead of 16.
///
/// # Examples
///
/// ```
/// use pdfcer_core::crypto::aes::{decrypt_cbc_256, MIN_CIPHERTEXT_LEN};
///
/// // Produced with key `[0x42; 32]` and IV `[0x24; 16]`.
/// let mut ciphertext: Vec<u8> = vec![0x24; 16];
/// ciphertext.extend_from_slice(&[
///     0x17, 0x18, 0xb1, 0xdf, 0xc1, 0xf1, 0x47, 0xfd, 0xf8, 0x2f, 0x6e, 0xd0,
///     0x84, 0x45, 0xc4, 0x51, 0x2c, 0x86, 0x1b, 0x01, 0x3c, 0x80, 0x8c, 0x92,
///     0x88, 0x51, 0xc3, 0xc7, 0x71, 0xb5, 0xdf, 0x35, 0x06, 0x20, 0xbc, 0xec,
///     0x61, 0x3c, 0x8e, 0x33, 0x69, 0x63, 0x85, 0x99, 0x70, 0xe8, 0x76, 0xbf,
/// ]);
///
/// let plain = decrypt_cbc_256(&[0x42; 32], &ciphertext);
/// assert_eq!(plain, b"hello world! this is my plaintext.");
/// assert!(ciphertext.len() >= MIN_CIPHERTEXT_LEN);
/// ```
///
/// A 16-byte key is refused rather than silently widened:
///
/// ```
/// use pdfcer_core::crypto::aes::decrypt_cbc_256;
/// assert!(decrypt_cbc_256(&[0x42; 16], &[0u8; 48]).is_empty());
/// ```
#[must_use]
pub fn decrypt_cbc_256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let Ok(key): Result<[u8; KEY_LEN_256], _> = key.try_into() else {
        return Vec::new();
    };
    let Some((iv, body)) = split_iv_and_whole_blocks(data) else {
        return Vec::new();
    };

    let blocks = blocks_of(body);
    let mut out = vec![AesBlock::default(); blocks.len()];

    let mut dec = Aes256CbcDec::new(&key.into(), &iv.into());
    if dec.decrypt_blocks_b2b(&blocks, &mut out).is_err() {
        return Vec::new();
    }

    let mut plain = flatten(out);
    strip_pkcs7(&mut plain);
    plain
}

/// Unwrap `/UE` or `/OE` — AES-256-CBC with a **zero IV** and **no padding**.
///
/// **Adobe ExtensionLevel 3, Algorithms 3.8 (e), 3.9 (d) and 3.2a**, all of
/// which say the same thing in the same words: "**CBC mode with no padding and
/// an initialization vector of zero**". `key` is the 32-byte intermediate key
/// `SHA-256(password ‖ KeySalt [‖ U])`; `wrapped` is the raw `/UE` or `/OE`
/// string; the result is the 32-byte **file encryption key**.
///
/// # Why this is a separate function from [`decrypt_cbc_256`]
///
/// Because every difference between them is silent (**T25**), and two of the
/// three would even produce a plausible 32-byte-ish answer:
///
/// - [`decrypt_cbc_256`] reads the **first 16 bytes as an IV**. Pointed at a
///   32-byte `/UE` it would consume half the ciphertext as an IV and return a
///   16-byte result — a "file encryption key" of the wrong length, which then
///   fails a length check somewhere far from here, if anything checks at all.
/// - A non-zero IV corrupts **exactly the first 16 bytes** of the recovered
///   key and leaves the second 16 perfect. The document then decrypts to
///   noise, and the half-correct key is invisible from the outside.
/// - PKCS#7 stripping would look at the last recovered key byte and, roughly
///   one time in sixteen, discard between 1 and 16 bytes of a key that is
///   **uniformly random and therefore has no padding to strip**. That failure
///   is intermittent by construction — the same code would work on most files
///   and fail on some.
///
/// # Returns
///
/// The unwrapped bytes: exactly `wrapped.len()` of them, rounded down to a
/// whole number of blocks. An empty `Vec` when `wrapped` is not a whole number
/// of 16-byte blocks or is empty. Callers must check the length before using
/// the result as a key — this function does not know what is being unwrapped.
///
/// # Examples
///
/// The real `/UE` from `fixtures/synthetic/encryption/enc-aes-256-r5.pdf`,
/// unwrapped with the intermediate key derived from its user password
/// `userpw`:
///
/// ```
/// use pdfcer_core::crypto::aes::unwrap_key_cbc_256;
///
/// // SHA-256(b"userpw" || UserKeySalt), where the salt is /U bytes 40..48.
/// let intermediate = [
///     0x66, 0xbf, 0xb4, 0x14, 0xd4, 0x2f, 0x0d, 0x23, 0xf6, 0xdd, 0xd8, 0x65,
///     0x58, 0x44, 0x2d, 0x20, 0x3c, 0x8b, 0x36, 0xce, 0xee, 0xdf, 0x16, 0x3d,
///     0xb3, 0x18, 0xd5, 0xdf, 0x5d, 0xcf, 0x8f, 0x6e,
/// ];
/// let ue = [
///     0xe2, 0x24, 0xdc, 0x92, 0x32, 0x71, 0x44, 0xdf, 0x5a, 0xb4, 0x75, 0xa1,
///     0x00, 0x8b, 0x1d, 0x37, 0xef, 0x90, 0xe7, 0x8a, 0x85, 0x36, 0x58, 0xe5,
///     0xf7, 0x77, 0x06, 0xc2, 0x1e, 0x90, 0xb3, 0x8a,
/// ];
///
/// let file_key = unwrap_key_cbc_256(&intermediate, &ue);
/// assert_eq!(file_key.len(), 32, "no padding is stripped");
/// assert_eq!(
///     file_key,
///     vec![
///         0x9c, 0x41, 0xcb, 0x1c, 0x02, 0x13, 0x80, 0x48, 0x96, 0x0d, 0x57, 0xf4,
///         0x8a, 0x21, 0x22, 0xf8, 0xfe, 0xbf, 0x4a, 0x68, 0xdd, 0x03, 0x18, 0x62,
///         0xaf, 0x90, 0xba, 0xc8, 0x20, 0x51, 0xf6, 0x0c,
///     ]
/// );
/// ```
#[must_use]
pub fn unwrap_key_cbc_256(key: &[u8; KEY_LEN_256], wrapped: &[u8]) -> Vec<u8> {
    if wrapped.is_empty() || !wrapped.len().is_multiple_of(BLOCK_LEN) {
        return Vec::new();
    }

    let blocks = blocks_of(wrapped);
    let mut out = vec![AesBlock::default(); blocks.len()];

    // The zero IV, written as a value rather than defaulted implicitly: it is
    // a normative parameter of Algorithms 3.8/3.9/3.2a, not an absence.
    let zero_iv = [0u8; IV_LEN];
    let mut dec = Aes256CbcDec::new(&(*key).into(), &zero_iv.into());
    if dec.decrypt_blocks_b2b(&blocks, &mut out).is_err() {
        return Vec::new();
    }

    // No `strip_pkcs7`. See the doc comment: the plaintext is a random key,
    // and stripping would occasionally eat part of it.
    flatten(out)
}

/// Decrypt one 16-byte block with AES-256 in **ECB** — the `/Perms` operation,
/// and the only place in PDF where ECB appears.
///
/// **Adobe ExtensionLevel 3, Algorithms 3.10 and 3.13.** `key` is the file
/// encryption key; `block` is the 16-byte `/Perms` string. ECB over a single
/// block is exactly the raw block cipher, which is why this takes `Aes256`'s
/// `decrypt_block` directly instead of a mode wrapper — there is no chaining
/// to do and no dependency to add for it.
///
/// # There is no IV parameter, and that is the point
///
/// Adobe's text reads "AES-256 in ECB mode with an initialization vector of
/// zero", which is a defect: ECB takes no IV. ISO 32000-2's public errata
/// delete the phrase from all three places it appears (Algorithms 2.A (f), 10
/// (f) and 13 (a)). Nothing about the *output* changes — a zero IV is what ECB
/// does anyway — so this signature is the entire benefit of knowing the
/// correction: there is no parameter here to pass a stray IV to, and no reader
/// of this code has to wonder whether one was forgotten. Source:
/// `iso32000__delta__pdf20_encryption.md` § D6.
///
/// ECB is unsafe for general data — identical plaintext blocks encrypt to
/// identical ciphertext blocks — but `/Perms` is a *single* block, so the mode
/// has no repetition to leak. That is presumably why the supplement chose it,
/// and it is the reason no wider use of ECB is or should be reachable from
/// this crate.
///
/// # Examples
///
/// The real `/Perms` from `fixtures/synthetic/encryption/enc-aes-256-r5.pdf`,
/// decrypted with that document's file encryption key:
///
/// ```
/// use pdfcer_core::crypto::aes::decrypt_ecb_256_block;
///
/// let file_key = [
///     0x9c, 0x41, 0xcb, 0x1c, 0x02, 0x13, 0x80, 0x48, 0x96, 0x0d, 0x57, 0xf4,
///     0x8a, 0x21, 0x22, 0xf8, 0xfe, 0xbf, 0x4a, 0x68, 0xdd, 0x03, 0x18, 0x62,
///     0xaf, 0x90, 0xba, 0xc8, 0x20, 0x51, 0xf6, 0x0c,
/// ];
/// let perms = [
///     0x71, 0x6a, 0xf6, 0xa5, 0x5e, 0xa2, 0xaf, 0xb6,
///     0xa9, 0xb8, 0x8a, 0xe3, 0x6e, 0x38, 0xb5, 0xe1,
/// ];
///
/// let out = decrypt_ecb_256_block(&file_key, &perms);
/// // Bytes 9, 10 and 11 are the literal marker "adb" (Algorithm 3.13).
/// assert_eq!(&out[9..12], b"adb");
/// // Bytes 0..4, little-endian, are the permission flags: 0xFFFFFFFC.
/// assert_eq!(u32::from_le_bytes([out[0], out[1], out[2], out[3]]), 0xFFFF_FFFC);
/// // Byte 8 is 'T' or 'F' — the /EncryptMetadata boolean.
/// assert_eq!(out[8], b'T');
/// ```
#[must_use]
pub fn decrypt_ecb_256_block(key: &[u8; KEY_LEN_256], block: &[u8; BLOCK_LEN]) -> [u8; BLOCK_LEN] {
    let cipher = Aes256::new(&(*key).into());
    let mut b = AesBlock::from(*block);
    cipher.decrypt_block(&mut b);
    b.into()
}

/// Remove PKCS#7 padding **if and only if** it verifies, leaving the buffer
/// untouched otherwise.
///
/// §7.6.2 mandates the padding, so a conforming file always has it and always
/// takes the stripping branch. The non-verifying branch exists for damaged and
/// non-conforming files, and returning the bytes unstripped is what lets a
/// self-terminating filter like `/FlateDecode` still recover the stream — see
/// the module docs for the full argument, including why the padding-oracle
/// objection does not apply to a local file reader.
///
/// A valid pad is a final byte `n` in `1..=16` whose value is repeated in all
/// `n` trailing bytes, and which does not exceed the buffer.
fn strip_pkcs7(buf: &mut Vec<u8>) {
    let Some(&last) = buf.last() else { return };
    let n = usize::from(last);

    // 0 is never a valid pad length, and a pad longer than one block -- or
    // longer than the data -- is malformed.
    if n == 0 || n > BLOCK_LEN || n > buf.len() {
        return;
    }
    // Every one of the n trailing bytes must equal n. Checking only the last
    // byte would strip a plaintext that merely happens to end in 0x01.
    //
    // `.get()` rather than a slice index: the guard above already proves
    // `n <= buf.len()`, but this function decides how many bytes of a
    // possibly-hostile file to discard, and a checked access costs nothing to
    // keep the proof local instead of two statements away.
    let tail_start = buf.len() - n;
    if buf
        .get(tail_start..)
        .is_some_and(|tail| tail.iter().all(|&b| usize::from(b) == n))
    {
        buf.truncate(tail_start);
    }
}

#[cfg(test)]
// Tests slice and `expect` against fixtures they construct three lines above,
// where a panic IS the failure report and a checked access would only convert
// a precise line number into a silent `None`. The crate-level bans exist for
// the library's untrusted-input paths, which is not what any of this is.
#[allow(clippy::indexing_slicing, clippy::expect_used)]
mod tests {
    use super::*;
    use aes::cipher::BlockModeEncrypt;

    type Aes128CbcEnc = cbc::Encryptor<Aes128>;

    /// Encrypt the way a conforming producer would: random-looking IV in
    /// front, PKCS#7 padded body behind. Used to build ciphertexts whose
    /// plaintext is known, so the assertions below are about *this* module's
    /// framing rather than about AES itself.
    ///
    /// **The padding is applied by hand rather than with `cbc`'s `Pkcs7`
    /// helper**, for two reasons. It keeps the `block-padding` feature off in
    /// the real dependency (R24's spirit: no feature enabled that the shipping
    /// code does not need). And it means [`strip_pkcs7`] is being checked
    /// against a pad this test wrote out explicitly — §7.6.2's rule, spelled
    /// as code — rather than against the same library's opinion of the same
    /// rule, which would agree with itself even if both were wrong.
    fn encrypt(key: &[u8; 16], iv: &[u8; 16], plain: &[u8]) -> Vec<u8> {
        // PKCS#7: append `n` bytes of value `n`, where n is 1..=16 chosen so
        // the result is block-aligned. Note n is never 0 -- an already-aligned
        // plaintext gains a whole extra block.
        let n = BLOCK_LEN - (plain.len() % BLOCK_LEN);
        let mut buf = plain.to_vec();
        buf.extend(std::iter::repeat_n(
            u8::try_from(n).expect("n is 1..=16"),
            n,
        ));

        let mut blocks: Vec<Block<Aes128>> = buf
            .chunks_exact(BLOCK_LEN)
            .map(|c| {
                let mut b = Block::<Aes128>::default();
                b.copy_from_slice(c);
                b
            })
            .collect();
        Aes128CbcEnc::new(key.into(), iv.into()).encrypt_blocks(&mut blocks);

        let mut out = iv.to_vec();
        out.extend(blocks.into_iter().flatten());
        out
    }

    /// The whole point of the module: a conforming ciphertext round-trips to
    /// exactly its plaintext, IV and padding both removed.
    #[test]
    fn round_trips_a_conforming_ciphertext() {
        let key = [0x42u8; 16];
        let iv = [0x24u8; 16];
        for plain in [
            &b""[..],
            b"a",
            b"exactly sixteen!",
            b"hello world! this is my plaintext.",
            &[0xFFu8; 1000][..],
        ] {
            let ct = encrypt(&key, &iv, plain);
            assert_eq!(
                decrypt_cbc_128(&key, &ct),
                plain,
                "plain len {}",
                plain.len()
            );
        }
    }

    /// T5, stated as an assertion rather than a comment: the ciphertext is
    /// always at least 17 bytes longer than the plaintext, which is the fact
    /// that forces the object model to record a shortened span.
    #[test]
    fn ciphertext_is_always_at_least_17_bytes_longer() {
        let (key, iv) = ([0x01u8; 16], [0x02u8; 16]);
        for len in [0usize, 1, 15, 16, 17, 31, 32, 33] {
            let plain = vec![0xABu8; len];
            let ct = encrypt(&key, &iv, &plain);
            assert!(
                ct.len() > plain.len() + IV_LEN,
                "len {len}: ct {} vs plain {}",
                ct.len(),
                plain.len()
            );
            assert_eq!(decrypt_cbc_128(&key, &ct).len(), len);
        }
    }

    /// A plaintext that is already a whole number of blocks still gets a
    /// FULL block of padding. Getting this wrong strips 16 real bytes off
    /// every such stream -- and only off those, so it hides well.
    #[test]
    fn a_block_aligned_plaintext_still_carries_a_full_pad_block() {
        let (key, iv) = ([0x03u8; 16], [0x04u8; 16]);
        let plain = b"exactly sixteen!";
        let ct = encrypt(&key, &iv, plain);
        assert_eq!(ct.len(), IV_LEN + 32, "16 bytes of data + 16 of pad");
        assert_eq!(decrypt_cbc_128(&key, &ct), plain);
    }

    /// The IV is data, not configuration. Decrypting with the IV omitted --
    /// the mistake a reader makes when it looks for an `/IV` dictionary key
    /// and finds none -- corrupts the first block and leaves the rest intact,
    /// which is exactly why the bug survives casual testing.
    #[test]
    fn the_iv_comes_from_the_data_not_a_zero_default() {
        let (key, iv) = ([0x05u8; 16], [0x77u8; 16]);
        let plain = b"the first sixteen bytes are the ones that break";
        let ct = encrypt(&key, &iv, plain);

        assert_eq!(decrypt_cbc_128(&key, &ct), plain);

        // Same body, but a zero IV substituted: only block 0 differs.
        let mut zeroed = vec![0u8; IV_LEN];
        zeroed.extend_from_slice(&ct[IV_LEN..]);
        let wrong = decrypt_cbc_128(&key, &zeroed);
        assert_ne!(&wrong[..BLOCK_LEN], &plain[..BLOCK_LEN]);
        assert_eq!(&wrong[BLOCK_LEN..], &plain[BLOCK_LEN..]);
    }

    /// The documented leniency, direction one: padding that verifies is gone.
    #[test]
    fn valid_padding_is_stripped() {
        let mut b = b"data\x03\x03\x03".to_vec();
        strip_pkcs7(&mut b);
        assert_eq!(b, b"data");

        let mut full = vec![0x10u8; 16];
        strip_pkcs7(&mut full);
        assert!(full.is_empty(), "a full block of 0x10 is all padding");
    }

    /// The documented leniency, direction two: padding that does not verify
    /// costs nothing. Without this test the lenient branch could be deleted
    /// and every other test here would still pass.
    #[test]
    fn invalid_padding_keeps_every_byte() {
        // Last byte says 3, but the three trailing bytes are not all 3.
        let mut mismatched = b"data\x01\x02\x03".to_vec();
        strip_pkcs7(&mut mismatched);
        assert_eq!(mismatched, b"data\x01\x02\x03");

        // 0 is never a valid pad length.
        let mut zero = b"data\x00".to_vec();
        strip_pkcs7(&mut zero);
        assert_eq!(zero, b"data\x00");

        // A pad claiming more bytes than exist.
        let mut over = b"\x09\x09".to_vec();
        strip_pkcs7(&mut over);
        assert_eq!(over, b"\x09\x09");

        // Longer than one block is malformed even though the bytes agree.
        let mut long = vec![0x11u8; 17];
        strip_pkcs7(&mut long);
        assert_eq!(long.len(), 17);
    }

    /// A plaintext that merely *ends* in bytes resembling padding must not be
    /// truncated. This is the case a last-byte-only check gets wrong.
    #[test]
    fn a_plaintext_ending_in_one_is_not_mistaken_for_padding() {
        let (key, iv) = ([0x06u8; 16], [0x07u8; 16]);
        let plain = b"value\x01";
        let ct = encrypt(&key, &iv, plain);
        assert_eq!(decrypt_cbc_128(&key, &ct), plain);
    }

    /// Malformed inputs degrade to "no plaintext" instead of panicking. A
    /// crate that parses untrusted files must not abort its host.
    #[test]
    fn malformed_input_yields_no_plaintext_and_never_panics() {
        let key = [0x08u8; 16];
        assert!(decrypt_cbc_128(&key, b"").is_empty());
        assert!(
            decrypt_cbc_128(&key, &[0u8; 31]).is_empty(),
            "under the minimum"
        );
        assert!(
            decrypt_cbc_128(&key, &[0u8; 16]).is_empty(),
            "IV but no body"
        );
        // A key of the wrong length is refused rather than padded or truncated.
        assert!(decrypt_cbc_128(&[0u8; 5], &[0u8; 64]).is_empty());
        assert!(decrypt_cbc_128(&[0u8; 32], &[0u8; 64]).is_empty());
    }

    type Aes256CbcEnc = cbc::Encryptor<Aes256>;

    /// AES-256's counterpart to [`encrypt`], for the `/AESV3` framing tests.
    /// Padding applied by hand, for the same reason as above.
    fn encrypt_256(key: &[u8; 32], iv: &[u8; 16], plain: &[u8]) -> Vec<u8> {
        let n = BLOCK_LEN - (plain.len() % BLOCK_LEN);
        let mut buf = plain.to_vec();
        buf.extend(std::iter::repeat_n(
            u8::try_from(n).expect("n is 1..=16"),
            n,
        ));
        let mut blocks = blocks_of(&buf);
        Aes256CbcEnc::new(key.into(), iv.into()).encrypt_blocks(&mut blocks);
        let mut out = iv.to_vec();
        out.extend(flatten(blocks));
        out
    }

    /// `/AESV3` document data round-trips, at the same lengths `/AESV2` is
    /// checked at. Same framing, longer key.
    #[test]
    fn aes_256_round_trips_a_conforming_ciphertext() {
        let key = [0x42u8; 32];
        let iv = [0x24u8; 16];
        for plain in [
            &b""[..],
            b"a",
            b"exactly sixteen!",
            b"hello world! this is my plaintext.",
            &[0xFFu8; 1000][..],
        ] {
            let ct = encrypt_256(&key, &iv, plain);
            assert_eq!(
                decrypt_cbc_256(&key, &ct),
                plain,
                "plain len {}",
                plain.len()
            );
        }
    }

    /// ★ The two key lengths are **not** interchangeable, in either direction.
    ///
    /// A 16-byte key handed to [`decrypt_cbc_256`] and a 32-byte key handed to
    /// [`decrypt_cbc_128`] are both refused outright rather than padded,
    /// truncated or reinterpreted. Without this, a `/V` 4 document that named
    /// `/AESV3` would silently decrypt every object to nothing — which is why
    /// [`EncryptionConfig::parse`] refuses that combination *and* why the
    /// refusal is duplicated here.
    ///
    /// [`EncryptionConfig::parse`]: super::standard::EncryptionConfig::parse
    #[test]
    fn the_two_key_lengths_are_not_interchangeable() {
        let ct256 = encrypt_256(&[0x11u8; 32], &[0x22u8; 16], b"some plaintext");
        assert!(
            decrypt_cbc_256(&[0x11u8; 16], &ct256).is_empty(),
            "AES-256 must refuse a 16-byte key rather than widening it"
        );

        let ct128 = encrypt(&[0x11u8; 16], &[0x22u8; 16], b"some plaintext");
        assert!(
            decrypt_cbc_128(&[0x11u8; 32], &ct128).is_empty(),
            "AES-128 must refuse a 32-byte key rather than truncating it"
        );

        // And the right key at the right length recovers the plaintext, so the
        // two refusals above are about the LENGTH and not about the data.
        assert_eq!(decrypt_cbc_256(&[0x11u8; 32], &ct256), b"some plaintext");
        assert_eq!(decrypt_cbc_128(&[0x11u8; 16], &ct128), b"some plaintext");
    }

    /// ★ T25, direction one: the `/UE`/`/OE` unwrap must **not** strip
    /// padding, and must **not** take an IV from the data.
    ///
    /// A wrapped key is 32 uniformly random bytes. Feeding it to
    /// [`decrypt_cbc_256`] — the natural mistake, since it is right there and
    /// takes the same key — consumes half of it as an IV and returns 16 bytes
    /// at most. [`unwrap_key_cbc_256`] returns all 32.
    #[test]
    fn the_key_unwrap_keeps_all_32_bytes_where_the_data_path_would_not() {
        let key = [0x33u8; 32];
        // Two blocks of ciphertext, i.e. a wrapped 32-byte key.
        let wrapped = [0x5Au8; 32];

        let unwrapped = unwrap_key_cbc_256(&key, &wrapped);
        assert_eq!(unwrapped.len(), 32, "no IV consumed, no padding stripped");

        let as_data = decrypt_cbc_256(&key, &wrapped);
        assert!(
            as_data.len() < 32,
            "the data path consumes 16 bytes as an IV and may strip padding on \
             top: {} bytes",
            as_data.len()
        );
        assert_ne!(
            as_data, unwrapped,
            "if these ever agree, this test is not observing the difference it \
             claims to"
        );
    }

    /// ★ T25, direction two: the unwrap IV is **zero**, and a non-zero one
    /// corrupts exactly the first block.
    ///
    /// That is the shape that makes the mistake survive testing: 16 of the 32
    /// key bytes come back perfect, so nothing about the result looks like
    /// noise, and the document simply decrypts to garbage far away from here.
    #[test]
    fn a_non_zero_unwrap_iv_corrupts_only_the_first_16_key_bytes() {
        let key = [0x44u8; 32];
        let wrapped = [0x77u8; 32];

        let right = unwrap_key_cbc_256(&key, &wrapped);

        // The same operation with a non-zero IV, computed here rather than by
        // breaking the real function.
        let blocks = blocks_of(&wrapped);
        let mut out = vec![AesBlock::default(); blocks.len()];
        let mut dec = Aes256CbcDec::new(&key.into(), &[0x01u8; 16].into());
        dec.decrypt_blocks_b2b(&blocks, &mut out)
            .expect("output is exactly the right size");
        let wrong = flatten(out);

        assert_eq!(wrong.len(), right.len());
        assert_ne!(&wrong[..16], &right[..16], "block 0 is corrupted");
        assert_eq!(
            &wrong[16..],
            &right[16..],
            "and block 1 is IDENTICAL — which is why this mistake hides"
        );
    }

    /// ★ T25, direction three: the unwrap must **not strip PKCS#7**, even
    /// when the recovered key happens to end in bytes that look exactly like
    /// valid padding.
    ///
    /// # This test exists because its absence was measured
    ///
    /// The padding-stripping mistake was made deliberately, in the real
    /// function, to see what would go red. **Nothing did** — 70 crypto unit
    /// tests, 20 end-to-end decryption tests and the CLI's byte-identical
    /// render comparison all stayed green, including the qpdf published-key
    /// vector. The reason is the whole hazard: a file encryption key is 32
    /// uniformly random bytes, so its last byte is in `1..=16` only about one
    /// time in sixteen, and the *trailing* bytes must then all repeat it,
    /// which for a 1-byte pad is automatic and for longer pads is
    /// vanishingly rare. Every key in the fixture corpus happens to end in
    /// something above 0x10.
    ///
    /// So the bug is **intermittent by construction**: the same code opens
    /// most documents perfectly and silently produces a short key for a few,
    /// which then decrypt to noise. No amount of fixture collecting reliably
    /// finds it. The only way to test for it is to build a key that ends the
    /// wrong way on purpose, which is what this does — for a 1-byte pad, and
    /// again for a 4-byte one, so a checker that verified only the final byte
    /// is caught too.
    ///
    /// (The [`crypto_decrypt` fuzz target] carries the same invariant as an
    /// assertion, which would find this eventually. "Eventually" is not a
    /// gate; this is.)
    ///
    /// [`crypto_decrypt` fuzz target]: https://github.com/KenM76/pdfcer
    #[test]
    fn the_key_unwrap_keeps_a_key_that_ends_in_valid_padding_bytes() {
        let unwrap_key = [0x88u8; 32];

        for tail in [&[0x01u8][..], &[0x04, 0x04, 0x04, 0x04]] {
            // A 32-byte "file encryption key" whose final bytes form a
            // textbook-valid PKCS#7 pad.
            let mut file_key = [0xC3u8; KEY_LEN_256];
            let at = KEY_LEN_256 - tail.len();
            file_key[at..].copy_from_slice(tail);

            // Wrap it exactly as Algorithm 3.8 step (e) does: AES-256-CBC,
            // zero IV, NO padding.
            let mut blocks = blocks_of(&file_key);
            Aes256CbcEnc::new(&unwrap_key.into(), &[0u8; IV_LEN].into())
                .encrypt_blocks(&mut blocks);
            let wrapped = flatten(blocks);
            assert_eq!(wrapped.len(), KEY_LEN_256, "two blocks in, two blocks out");

            assert_eq!(
                unwrap_key_cbc_256(&unwrap_key, &wrapped),
                file_key.to_vec(),
                "a {}-byte PKCS#7-shaped tail must survive: the unwrap has no \
                 padding to strip, and eating these bytes yields a {}-byte key \
                 that authenticates fine and decrypts nothing",
                tail.len(),
                at
            );
        }
    }

    /// The unwrap refuses anything that is not a whole number of blocks,
    /// rather than silently dropping a partial one. A 31-byte `/UE` is a
    /// malformed document, not a 16-byte key.
    #[test]
    fn the_key_unwrap_refuses_a_non_block_multiple() {
        let key = [0x55u8; 32];
        for len in [0usize, 1, 15, 17, 31, 33] {
            assert!(
                unwrap_key_cbc_256(&key, &vec![0u8; len]).is_empty(),
                "{len} bytes is not a whole number of blocks"
            );
        }
        assert_eq!(unwrap_key_cbc_256(&key, &[0u8; 16]).len(), 16);
        assert_eq!(unwrap_key_cbc_256(&key, &[0u8; 32]).len(), 32);
    }

    /// ECB is deterministic and stateless: the same block under the same key
    /// always gives the same output, with nothing carried between calls.
    ///
    /// That property is exactly why ECB is wrong for general data and right
    /// for a single 16-byte `/Perms` block — there is no second block for the
    /// repetition to leak through.
    #[test]
    fn ecb_is_stateless_and_repeatable() {
        let key = [0x66u8; 32];
        let a = decrypt_ecb_256_block(&key, &[0x01u8; 16]);
        let b = decrypt_ecb_256_block(&key, &[0x02u8; 16]);
        let a_again = decrypt_ecb_256_block(&key, &[0x01u8; 16]);
        assert_eq!(a, a_again, "no state carries between calls");
        assert_ne!(a, b);

        // ECB over ONE block equals CBC with a zero IV over that block --
        // there is nothing to chain. This is why the ISO erratum striking
        // "with an initialization vector of zero" changes no behaviour, and it
        // is asserted so the claim in the module docs is checked rather than
        // merely stated.
        assert_eq!(a.to_vec(), unwrap_key_cbc_256(&key, &[0x01u8; 16]));
    }

    /// A trailing partial block is ignored, not fatal: the whole blocks in
    /// front of it still decrypt.
    #[test]
    fn a_trailing_partial_block_is_ignored() {
        let (key, iv) = ([0x09u8; 16], [0x0Au8; 16]);
        let plain = b"sixteen bytes ok";
        let mut ct = encrypt(&key, &iv, plain);
        ct.extend_from_slice(b"partial");
        assert_eq!(decrypt_cbc_128(&key, &ct), plain);
    }
}
