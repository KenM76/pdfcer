//! Cryptographically strong randomness for encryption **authoring** (`Pass 5.4`).
//!
//! One function, [`fill`], and a hard rule about it: it is the ONLY source of
//! the bytes that must be unpredictable in an encrypted document — the 256-bit
//! file encryption key (Algorithms 8/9 require it be "generated with a strong
//! random number generator"), the 8-byte validation and key salts, and the
//! 16-byte AES-CBC IV of every encrypted string and stream (Algorithm 1.A).
//!
//! # Why this is a seam and not a call to `getrandom` at each site
//!
//! Two reasons, and the second is the load-bearing one:
//!
//! 1. **Every caller must treat "no entropy" as a refusal, never a fallback.**
//!    A weak key is worse than no encryption, because it *looks* encrypted. So
//!    [`fill`] returns a `Result`, and there is exactly one place — here — where
//!    the decision "what counts as a strong source" is made.
//! 2. **The engine crate must cross into the wasm32 web fork, where there is no
//!    entropy source and no encryption authoring.** `getrandom` 0.2 does not
//!    even *compile* on `wasm32-unknown-unknown` without a backend feature, so
//!    the dependency is target-gated OFF wasm in `Cargo.toml` and this module
//!    provides a refusing stub there. The web fork is a viewer; it never saves,
//!    let alone encrypts, so the stub is unreachable in practice and correct in
//!    principle — a `RngUnavailable` is exactly the right answer to "encrypt
//!    this" on a target with no CSPRNG.

/// The one thing that can go wrong: the operating system's CSPRNG was not
/// available. On every real target this does not happen; it is modelled rather
/// than `unwrap`ped because a silently-weak key is the one failure this whole
/// module exists to make impossible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RngError {
    /// No cryptographically strong random source is reachable on this target.
    /// On `wasm32` this is unconditional (there is no CSPRNG and no encryption
    /// authoring); elsewhere it means the OS call itself failed.
    #[error("no cryptographically strong random source is available on this target")]
    Unavailable,
}

/// Fill `buf` with cryptographically strong random bytes.
///
/// # Errors
///
/// [`RngError::Unavailable`] if the OS CSPRNG could not be reached (never, on a
/// normal desktop target) or if this is a `wasm32` build (always — see the
/// module docs).
#[cfg(not(target_arch = "wasm32"))]
pub fn fill(buf: &mut [u8]) -> Result<(), RngError> {
    getrandom::getrandom(buf).map_err(|_| RngError::Unavailable)
}

/// The `wasm32` stub: there is no CSPRNG and no encryption authoring on the web
/// fork, so this refuses unconditionally. See the module docs.
///
/// # Errors
///
/// Always [`RngError::Unavailable`].
#[cfg(target_arch = "wasm32")]
pub fn fill(_buf: &mut [u8]) -> Result<(), RngError> {
    Err(RngError::Unavailable)
}

/// A fresh `[u8; N]` of strong random bytes.
///
/// # Errors
///
/// As [`fill`].
pub fn array<const N: usize>() -> Result<[u8; N], RngError> {
    let mut out = [0u8; N];
    fill(&mut out)?;
    Ok(out)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn two_draws_differ_and_are_not_all_zero() {
        let a: [u8; 32] = array().expect("desktop has a CSPRNG");
        let b: [u8; 32] = array().expect("desktop has a CSPRNG");
        assert_ne!(a, b, "two 32-byte draws must not collide");
        assert_ne!(
            a, [0u8; 32],
            "a key of all zeros is the failure this guards"
        );
    }
}
