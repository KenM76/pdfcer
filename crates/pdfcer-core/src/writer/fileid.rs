//! # `/ID[1]` — the changing file identifier (ISO 32000-1 §14.4)
//!
//! One job: produce the 16 bytes that replace `ID[1]` when — and only
//! when — a save writes at least one changed object. The *when* lives in
//! [`super::DirtySet::changes_content`] and [`super::save`]; this module
//! is only the *what*.
//!
//! Spec source: `iso32000__s__14.4.md` in the PDF-spec RAG.
//!
//! ## What §14.4 actually requires (and how little that is)
//!
//! > "To help ensure the uniqueness of file identifiers, they **should**
//! > be computed by means of a **message digest algorithm such as MD5**
//! > … using the following information: the current time; a string
//! > representation of the file's location, usually a pathname; the size
//! > of the file in bytes; the values of all entries in the file's
//! > document information dictionary."
//!
//! Everything in that sentence is `should`-strength, and the clause's
//! own NOTE goes further:
//!
//! > "The calculation of the file identifier **need not be
//! > reproducible**; all that matters is that the identifier is
//! > **likely to be unique**."
//!
//! So there is no correct value — only a unique-enough one. No length is
//! specified (16 bytes is an MD5 artifact, not a requirement), no
//! encoding is specified (literal and hexadecimal byte strings are both
//! legal §7.3.4 forms), and **no validator can check an `/ID` for
//! correctness**. That freedom is what lets pdfcer make three deliberate
//! choices the spec's suggested recipe would not have made.
//!
//! ## Choice 1 — no clock, no pathname, no hostname
//!
//! §14.4's suggested inputs include the current time and *"a string
//! representation of the file's location, usually a pathname"*. pdfcer
//! uses **neither**, and both omissions are principled:
//!
//! - A pathname is user data. Hashing it does not make it not-user-data:
//!   the digest still varies with `C:\Users\<name>\…`, and a file saved
//!   twice from two directories would carry different identifiers for
//!   reasons that have nothing to do with its contents. Under
//!   `ARCHITECTURE.md` §1.1's privacy posture, the environment does not
//!   leak into the artifact.
//! - A clock makes output non-reproducible, which collides with R41's
//!   no-fingerprint discipline in spirit (two builds, two runs, two
//!   different files from identical inputs) and makes byte-comparison
//!   testing of the mutation writer impossible. The one property a clock
//!   buys — distinguishing two saves of *identical* content — is a
//!   property nobody needs: two saves of identical content **are** the
//!   same revision.
//!
//! ## Choice 2 — the digest is over what actually changed
//!
//! The value is derived from:
//!
//! 1. the base file's own `ID[1]` (so a chain of revisions never
//!    repeats a value it already used, even if a later revision reverts
//!    to earlier content);
//! 2. the base file's length (cheap revision discriminator);
//! 3. **the bytes of the object definitions this save is appending** —
//!    i.e. precisely the changed content.
//!
//! Input 3 is why this function takes the appended body bytes rather
//! than the finished file: `/ID` lives *in* the trailer, which lives *in*
//! the finished file, so digesting the finished file would be circular.
//! Digesting the object definitions — everything written before the
//! cross-reference section begins — is well-defined, non-circular, and
//! is the part that carries the operator's actual edit.
//!
//! ## Choice 3 — not MD5, and no dependency
//!
//! *"such as MD5"* is an example, not a mandate, and §14.4 attaches **no
//! security claim** to this digest (its NOTE disclaims reproducibility;
//! there is nothing to forge). Pulling a cryptographic hash crate in for
//! it would add a dependency, an attribution entry and a license
//! classification (`LEGAL.md` §6) to buy a property nobody asked for.
//!
//! ⚠️ **Do not let this reasoning leak into §7.6.** The MD5 in §14.4 and
//! the MD5 in the standard security handler are unrelated uses: §7.6's
//! algorithms are bit-exactly specified and cannot be substituted. A
//! "we replaced MD5" policy that escaped from this module into the Pass 5
//! crypt stage would produce files no other reader can decrypt.
//!
//! What is used instead: FNV-1a-64 to absorb the inputs, then two
//! `splitmix64` finalizations to expand the 64-bit state to 16 bytes.
//! Both are well-known, public-domain, ~5-line integer mixers with good
//! avalanche behaviour — far more than *"likely to be unique"* needs.

/// FNV-1a 64-bit offset basis.
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64-bit prime.
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// `splitmix64`'s increment (the golden-ratio odd constant).
const GOLDEN_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

/// Compute the 16 bytes for `ID[1]` from the parts that identify this
/// revision.
///
/// The three inputs are documented in the module header; they are taken
/// as separate arguments rather than one pre-concatenated buffer so a
/// caller cannot accidentally digest the *finished file* (which would be
/// circular, since `/ID` is inside it).
///
/// Length-prefixing each part before absorbing it makes the digest
/// unambiguous: without it, `("ab", "c")` and `("a", "bc")` would hash
/// alike, which is a real (if unlikely) collision source when one input
/// is attacker-influenced document content.
///
/// # Examples
///
/// ```
/// use pdfcer_core::writer::fileid::changing_identifier;
///
/// let a = changing_identifier(b"prev-id", 1024, b"1 0 obj\n<< /Rotate 90 >>\nendobj\n");
/// let b = changing_identifier(b"prev-id", 1024, b"1 0 obj\n<< /Rotate 180 >>\nendobj\n");
/// assert_ne!(a, b, "different appended content must give a different identifier");
/// assert_eq!(a.len(), 16);
///
/// // Deterministic: the same revision of the same file yields the same
/// // value, which is what makes the writer byte-comparison-testable.
/// assert_eq!(a, changing_identifier(b"prev-id", 1024, b"1 0 obj\n<< /Rotate 90 >>\nendobj\n"));
/// ```
#[must_use]
pub fn changing_identifier(previous_id: &[u8], base_len: usize, appended: &[u8]) -> [u8; 16] {
    let mut h = FNV_OFFSET_BASIS;
    for part in [previous_id, &base_len.to_be_bytes(), appended] {
        h = absorb(h, &(part.len() as u64).to_be_bytes());
        h = absorb(h, part);
    }

    // Two independent 64-bit finalizations. Using the same state with
    // two different gammas rather than hashing twice keeps this O(n)
    // in the (potentially large) appended buffer.
    let lo = splitmix64(h);
    let hi = splitmix64(h ^ GOLDEN_GAMMA);

    let mut out = [0u8; 16];
    let (first, second) = out.split_at_mut(8);
    first.copy_from_slice(&hi.to_be_bytes());
    second.copy_from_slice(&lo.to_be_bytes());
    out
}

/// Absorb `bytes` into an FNV-1a-64 state.
fn absorb(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// `splitmix64`'s finalizer — the standard three-step xor-shift-multiply
/// avalanche. Public-domain (Vigna), reproduced here rather than
/// depended upon because it is five lines.
const fn splitmix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(GOLDEN_GAMMA);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn identifier_is_sixteen_bytes_and_deterministic() {
        // Determinism is a design property, not an accident: the
        // mutation writer's byte-comparison tests depend on it.
        let a = changing_identifier(b"seed", 10, b"body");
        let b = changing_identifier(b"seed", 10, b"body");
        assert_eq!(a, b);
        assert_eq!(a.len(), 16);
    }

    #[test]
    fn every_input_changes_the_identifier() {
        let base = changing_identifier(b"seed", 10, b"body");
        assert_ne!(base, changing_identifier(b"seeD", 10, b"body"));
        assert_ne!(base, changing_identifier(b"seed", 11, b"body"));
        assert_ne!(base, changing_identifier(b"seed", 10, b"bodz"));
    }

    #[test]
    fn length_prefixing_prevents_boundary_collisions() {
        // Without length prefixes ("ab", "") and ("a", "b") would be
        // indistinguishable after concatenation.
        assert_ne!(
            changing_identifier(b"ab", 0, b""),
            changing_identifier(b"a", 0, b"b")
        );
    }

    #[test]
    fn halves_are_independent() {
        // A bug that wrote the same 8 bytes twice would halve the space
        // and would not be caught by any test above.
        let id = changing_identifier(b"seed", 10, b"body");
        assert_ne!(
            id.get(..8).unwrap(),
            id.get(8..).unwrap(),
            "the two 64-bit halves must not be the same value"
        );
    }

    #[test]
    fn empty_inputs_are_accepted() {
        // A base file with no /ID never reaches this function (see the
        // writer's module docs), but an empty appended body is possible
        // in principle and must not panic or produce all-zero bytes.
        let id = changing_identifier(b"", 0, b"");
        assert_ne!(id, [0u8; 16]);
    }
}
