//! Fuzz target: the `/AESV2` and `/AESV3` decryption paths
//! (`pdfcer_core::crypto::aes`), plus `/R` 5's password preparation
//! (`pdfcer_core::crypto::r5`).
//!
//! # Why this target exists, and why it did not before
//!
//! Increment 1's ciphers needed no fuzzing to be *safe*: RC4 is a keystream
//! XOR over a 256-byte permutation and MD5 is a fixed-size block loop, so
//! neither has an input-derived length, offset, or count anywhere in it.
//! Neither can index out of bounds no matter what bytes arrive.
//!
//! AES-128-CBC is the first cipher in this crate where **the ciphertext's own
//! bytes decide control flow**:
//!
//! - the first 16 bytes are consumed as an IV, so anything shorter than that
//!   is a split at an index the data chose;
//! - the remainder is chunked into 16-byte blocks, so a length that is not a
//!   multiple of the block size is a truncation the data chose;
//! - and the **last decrypted byte is read as a length and used to truncate
//!   the buffer** (PKCS#7). That is the sharpest edge in the module: a
//!   value between 0 and 255 arriving from decrypted attacker-controlled
//!   bytes, used as an offset. It is guarded — `n == 0`, `n > BLOCK_LEN`,
//!   `n > buf.len()` — and this target is what says the guard holds for
//!   inputs nobody thought to write down.
//!
//! Note the second-order property that makes fuzzing worth more here than the
//! unit tests: the pad length is read *after* decryption, so its value is
//! effectively random and **not** something a hand-written fixture can steer.
//! libFuzzer varying the key and IV varies the pad byte for free.
//!
//! # What increment 3 added, and what it did not
//!
//! AES-256 arrived with three new entry points. Only two of them have
//! input-derived control flow, and saying which is which is the whole
//! judgement:
//!
//! - [`decrypt_cbc_256`] — **driven.** Identical framing to the 128-bit
//!   routine (IV split, block chunking, PKCS#7 length byte), so identical
//!   hazards, at a key length the same input has to be able to reach.
//! - [`unwrap_key_cbc_256`] — **driven.** Its key is a fixed 32 bytes, but the
//!   *wrapped* slice is not: it rejects anything that is not a whole number of
//!   blocks, and that rejection is a length test on attacker-supplied bytes.
//!   It deliberately does **not** strip padding, which this target pins as a
//!   length equality rather than a comment.
//! - `decrypt_ecb_256_block` — **not driven, on purpose.** Both arguments are
//!   fixed-size arrays and the function has no branch in it at all. There is
//!   no input for libFuzzer to vary that the type system does not already
//!   fix, so a target for it would be coverage theatre. `validate_perms` is
//!   the same: fixed sizes, constant indices, no length arithmetic.
//! - [`PreparedPassword::new`] — **driven.** It truncates at 127 bytes, which
//!   is an input-derived slice, and it is reachable directly from an operator
//!   -supplied string that has been through no validation whatsoever.
//!
//! # What is driven
//!
//! 1. [`decrypt_cbc_128`] and [`decrypt_cbc_256`] over an arbitrary key **and**
//!    arbitrary ciphertext, with the split taken from the input so libFuzzer
//!    controls key length independently of data length. Wrong key lengths
//!    (0, 5, 15, 17, 32 for the 128 routine; anything but 32 for the 256 one)
//!    are reachable and are a documented refusal path, not a panic.
//! 2. The same calls at every length boundary that matters — 0, and each of
//!    the first few bytes around `IV_LEN` and `MIN_CIPHERTEXT_LEN` — because
//!    those are exactly the indices the splits use, and a corpus that only
//!    ever contains long inputs never exercises them.
//! 3. [`unwrap_key_cbc_256`] over the same bytes, with the invariant that it
//!    returns either nothing or a whole number of blocks equal in length to
//!    its input — never a padding-stripped short key.
//! 4. [`PreparedPassword::new`] over the same bytes, with the invariant that
//!    the prepared form never exceeds 127 bytes and is always a prefix of what
//!    was handed in.
//!
//! # Invariant
//!
//! For ANY key and ANY bytes: no panic, no abort, no unbounded work, and the
//! output is never longer than the input (the crate relies on that — the
//! decryption walk in `document.rs` writes the plaintext back into the
//! ciphertext's own byte span and would overwrite the following object if it
//! could grow). The length relation is asserted here rather than merely
//! documented, because it is a *memory-safety* precondition for the caller
//! and not just a property of the cipher.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::crypto::PreparedPassword;
use pdfcer_core::crypto::aes::{
    BLOCK_LEN, IV_LEN, KEY_LEN_256, MIN_CIPHERTEXT_LEN, decrypt_cbc_128, decrypt_cbc_256,
    unwrap_key_cbc_256,
};

/// Run both document-data decryptions and assert the caller's precondition.
///
/// `document.rs` writes the result at `span.start` and shortens the recorded
/// length; a result longer than the input would silently run into the next
/// object. The check is cheap and converts a would-be corruption into a
/// reported crash.
///
/// Both key lengths are driven from the same bytes rather than in separate
/// targets: the two routines share their entire framing, so a corpus that
/// reached an interesting length for one has reached it for the other, and
/// splitting them would halve the value of every input libFuzzer finds.
fn drive(key: &[u8], data: &[u8]) {
    for plain in [decrypt_cbc_128(key, data), decrypt_cbc_256(key, data)] {
        assert!(
            plain.len() <= data.len(),
            "decryption must never grow its input: {} -> {} (key {} bytes)",
            data.len(),
            plain.len(),
            key.len()
        );
    }

    // The /UE and /OE key unwrap. Its key length is fixed by the type, so the
    // only input it can be given is the wrapped slice -- taken here from the
    // same bytes, padded or truncated to 32 for the key.
    let mut unwrap_key = [0u8; KEY_LEN_256];
    for (slot, byte) in unwrap_key.iter_mut().zip(key.iter()) {
        *slot = *byte;
    }
    let unwrapped = unwrap_key_cbc_256(&unwrap_key, data);
    assert!(
        unwrapped.is_empty() || unwrapped.len() == data.len(),
        "the key unwrap must return every byte or none -- a short result means \
         padding was stripped from a random key: {} -> {}",
        data.len(),
        unwrapped.len()
    );
    assert!(
        unwrapped.len() % BLOCK_LEN == 0,
        "and it must always be a whole number of blocks: {}",
        unwrapped.len()
    );
}

fuzz_target!(|data: &[u8]| {
    // The first byte chooses where the key ends, so libFuzzer can vary key
    // length and ciphertext length independently. Everything is `saturating`
    // / checked so the harness itself cannot be the thing that panics.
    let Some((&split, rest)) = data.split_first() else {
        // Even the empty input is a real case: `buf.last()` is `None` and the
        // padding strip must return without touching anything.
        drive(&[], &[]);
        return;
    };

    let at = usize::from(split).min(rest.len());
    let (key, body) = rest.split_at(at);
    drive(key, body);

    // `/R` 5's password preparation. Not a cipher, but it slices at 127 bytes
    // on data that reaches it straight from a command-line argument, a
    // password file or a GUI text box, none of which is validated first.
    let prepared = PreparedPassword::new(rest);
    assert!(
        prepared.as_bytes().len() <= 127,
        "a prepared password must never exceed the 127 bytes /R 5 hashes: {}",
        prepared.as_bytes().len()
    );
    assert!(
        rest.starts_with(prepared.as_bytes()),
        "preparation may only truncate -- it must never reorder or substitute, \
         because pdfcer does not implement the SASLprep step that would"
    );

    // Boundary sweep. The corpus will drift toward whatever lengths happen to
    // be interesting for coverage, and that is usually NOT the handful of
    // indices the splits are written against. Pin them explicitly.
    for n in [
        0,
        1,
        IV_LEN - 1,
        IV_LEN,
        IV_LEN + 1,
        MIN_CIPHERTEXT_LEN - 1,
        MIN_CIPHERTEXT_LEN,
        MIN_CIPHERTEXT_LEN + 1,
    ] {
        if let Some(prefix) = body.get(..n) {
            drive(key, prefix);
        }
    }
});
