//! Operator-initiated downloads: fetch a **pinned** URL and verify it against
//! a **pinned SHA-256** before it is allowed to become a file on disk.
//!
//! # What this crate is for, and the boundary it sits on
//!
//! `pdfcer-core` and `pdfcer-render` are **network-free permanently and
//! gate-enforced** — the engine must never need a network to parse or render,
//! and both crates must cross into the wasm32 fork where no native HTTP stack
//! exists. So this crate exists on the other side of that line, and the shells
//! (`pdfcer`, `pdfce-gui`) depend on it while the engine never does.
//!
//! The line is the operator's own, stated 2026-08-13:
//!
//! > *"the no network rule was made too broad. all I meant by that is the
//! > software itself didn't rely on network technology to function which would
//! > bloat it and slow things down the way it does for other pdf software. it
//! > is fine to have download update or download addin capability."*
//!
//! **What the software needs to RUN, versus what the operator can ASK it to
//! fetch.** This crate is entirely the second.
//!
//! ## What that narrowing did NOT relax
//!
//! **No telemetry, no analytics, no crash reporting, no licence callback, no
//! silent phone-home.** Every function here is called because an operator
//! asked for something. Nothing in this crate runs on a timer, at startup, or
//! in the background, and adding such a thing needs `ARCHITECTURE.md` §1.1's
//! opt-in treatment and its own decision record — an operator narrowing one
//! clause is not consent to widen its neighbour.
//!
//! # Why verification is not optional, and is not merely integrity
//!
//! [`fetch_verified`] refuses to write a file whose hash does not match. That
//! is a **supply-chain** control, not a corruption check, and the difference
//! matters for how it behaves: a truncated download and a substituted file are
//! indistinguishable to the caller, so both are refused identically, and
//! **nothing is written to the destination path in either case.**
//!
//! The concrete reason it is here rather than left to callers:
//! `docs/ocr-engine-survey.md` recorded that the Hugging Face and S3 copies of
//! "the ocrs models" are **not byte-identical** — different filenames, one
//! 13,280 bytes smaller, one 124 bytes larger. *"The ocrs models"* is not one
//! thing. A fetch that trusted a URL alone would install weights nobody
//! tested, and would do it silently.
//!
//! # What this crate deliberately does NOT do
//!
//! - **No mirror fallback, no retry-other-URL.** A pinned artifact has one
//!   source; silently reaching for a second is how you end up running the copy
//!   that was not measured.
//! - **No "latest version" concept.** There is a URL and a hash, together, or
//!   there is nothing.
//! - **No execution of anything fetched.** `R13` clause 5 forbids it, and that
//!   clause is **unresolved** against the operator's "download addin
//!   capability" — an add-in is executed code. Until he rules, this crate
//!   moves bytes to disk and stops there.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// A pinned, verifiable artifact: where it comes from and what it must hash to.
///
/// Both fields together are the identity. Neither alone is — a URL can serve
/// different bytes over time, and a hash with no source cannot be obtained.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PinnedArtifact {
    /// The exact URL to fetch. **HTTPS is required** (see [`FetchError::InsecureUrl`]).
    pub url: String,
    /// Lowercase hex SHA-256 the downloaded bytes must equal.
    pub sha256: String,
    /// The file name to write, relative to the caller's chosen directory.
    ///
    /// Separate from the URL's last path segment on purpose: upstream names
    /// often carry content-addressed suffixes that are *their* versioning
    /// (`text-detection-ssfbcj81.rten`), and pdfcer pins by hash instead and
    /// wants a stable local name.
    pub file_name: String,
}

impl PinnedArtifact {
    /// A new pinned artifact.
    ///
    /// The `sha256` is lowercased here so a caller pasting an uppercase digest
    /// from a checksum tool does not get a spurious mismatch — a failure that
    /// looks exactly like a tampered file and would send someone hunting.
    #[must_use]
    pub fn new(
        url: impl Into<String>,
        sha256: impl AsRef<str>,
        file_name: impl Into<String>,
    ) -> Self {
        Self {
            url: url.into(),
            sha256: sha256.as_ref().to_ascii_lowercase(),
            file_name: file_name.into(),
        }
    }
}

/// A failure to fetch or verify. Every variant is a named, actionable outcome.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FetchError {
    /// This build was compiled without the `download` feature.
    ///
    /// **A named refusal, never a silent no-op** (rule 2 of the
    /// strippable-capability convention). Without it, an operator on a
    /// stripped build would read a generic failure as a network problem and
    /// debug their firewall.
    #[error(
        "this build was compiled without download support (the `download` feature is off) — \
         obtain {file_name} manually and place it in the target directory"
    )]
    FeatureUnsupported {
        /// The file that would have been fetched.
        file_name: String,
    },
    /// The URL is not HTTPS.
    ///
    /// Refused rather than downgraded: a plain-HTTP fetch of an artifact that
    /// is then verified by hash is *almost* safe, but the hash itself would
    /// have arrived over some other channel and this crate cannot see how.
    #[error("refusing a non-HTTPS URL: {url}")]
    InsecureUrl {
        /// The offending URL.
        url: String,
    },
    /// The transfer failed.
    #[error("could not download {url}: {reason}")]
    Transport {
        /// The URL attempted.
        url: String,
        /// What the client reported.
        reason: String,
    },
    /// The server answered, but not with success.
    #[error("{url} returned HTTP {status}")]
    HttpStatus {
        /// The URL attempted.
        url: String,
        /// The status code.
        status: u16,
    },
    /// ★ The downloaded bytes do not match the pinned hash.
    ///
    /// **Nothing was written.** This is a supply-chain refusal: a truncated
    /// transfer and a substituted file look identical here, so both are
    /// refused, and the message gives both digests so the operator can tell a
    /// stale pin from a bad download by comparing against the manifest.
    #[error(
        "SHA-256 mismatch for {file_name}: expected {expected}, got {actual} — \
         nothing was written"
    )]
    HashMismatch {
        /// The artifact's file name.
        file_name: String,
        /// The pinned digest.
        expected: String,
        /// What actually arrived.
        actual: String,
    },
    /// The download exceeded [`MAX_ARTIFACT_BYTES`].
    ///
    /// A guard against an endpoint that streams forever — the network
    /// counterpart of the output-size ceilings every filter decoder carries
    /// (`ARCHITECTURE.md` §10).
    #[error("{url} exceeded the {limit}-byte ceiling for a single artifact")]
    TooLarge {
        /// The URL attempted.
        url: String,
        /// The ceiling.
        limit: u64,
    },
    /// Writing the verified bytes failed.
    #[error("could not write {path}: {source}")]
    Write {
        /// The destination.
        path: PathBuf,
        /// The I/O error.
        source: std::io::Error,
    },
}

/// The largest single artifact this crate will download, in bytes (64 MiB).
///
/// Sized against what pdfcer actually fetches — the two `ocrs` models are
/// 2.5 MB and 9.7 MB — with generous headroom for a future model set, and far
/// below anything that would be a plausible legitimate download for a PDF
/// editor. It exists because a caller cannot otherwise bound what a remote
/// endpoint hands back.
pub const MAX_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024;

/// Verify `bytes` against `artifact`'s pinned digest.
///
/// Separate from the download on purpose, and **not** behind the `download`
/// feature: an operator who obtained a file by hand can check it against the
/// same manifest, and a build with the fetcher stripped is more useful with a
/// verifier than without one.
///
/// # Errors
///
/// [`FetchError::HashMismatch`] with both digests.
pub fn verify_bytes(artifact: &PinnedArtifact, bytes: &[u8]) -> Result<(), FetchError> {
    let actual = sha256_hex(bytes);
    if actual == artifact.sha256 {
        Ok(())
    } else {
        Err(FetchError::HashMismatch {
            file_name: artifact.file_name.clone(),
            expected: artifact.sha256.clone(),
            actual,
        })
    }
}

/// Lowercase hex SHA-256 of `bytes`.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().fold(String::new(), |mut s, b| {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Download `artifact` into `dir`, verifying it **before** anything is written.
///
/// Returns the path written.
///
/// # Order of operations, which is the security property
///
/// The bytes are held in memory, hashed, compared, and only then written. A
/// download-then-verify-then-delete implementation would leave an unverified
/// file on disk for a window — and if the process died in that window, would
/// leave it there permanently, looking exactly like a good one.
///
/// # Errors
///
/// [`FetchError`] — feature stripped, non-HTTPS URL, transport failure,
/// non-success status, size ceiling, **hash mismatch**, or a write failure.
///
/// # Examples
///
/// ```no_run
/// use pdfcer_fetch::{PinnedArtifact, fetch_verified};
///
/// let art = PinnedArtifact::new(
///     "https://example.invalid/text-detection.rten",
///     "614aafabf27c94d386f7aa036c967c2e47e4b9938fa11531ca8f5698c1ca4c36",
///     "text-detection.rten",
/// );
/// let path = fetch_verified(&art, std::path::Path::new("models/ocrs"))?;
/// println!("verified and written to {}", path.display());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[cfg_attr(not(feature = "download"), allow(unused_variables))]
pub fn fetch_verified(artifact: &PinnedArtifact, dir: &Path) -> Result<PathBuf, FetchError> {
    #[cfg(not(feature = "download"))]
    {
        Err(FetchError::FeatureUnsupported {
            file_name: artifact.file_name.clone(),
        })
    }

    #[cfg(feature = "download")]
    {
        if !artifact.url.starts_with("https://") {
            return Err(FetchError::InsecureUrl {
                url: artifact.url.clone(),
            });
        }

        let response = ureq::get(&artifact.url).call().map_err(|e| match &e {
            ureq::Error::StatusCode(code) => FetchError::HttpStatus {
                url: artifact.url.clone(),
                status: *code,
            },
            other => FetchError::Transport {
                url: artifact.url.clone(),
                reason: other.to_string(),
            },
        })?;

        let bytes = response
            .into_body()
            .with_config()
            .limit(MAX_ARTIFACT_BYTES)
            .read_to_vec()
            .map_err(|e| {
                // A body that trips the limit surfaces here; distinguish it so
                // "the endpoint streamed forever" does not read as "the
                // network failed".
                let msg = e.to_string();
                if msg.contains("limit") {
                    FetchError::TooLarge {
                        url: artifact.url.clone(),
                        limit: MAX_ARTIFACT_BYTES,
                    }
                } else {
                    FetchError::Transport {
                        url: artifact.url.clone(),
                        reason: msg,
                    }
                }
            })?;

        // ★ Verify BEFORE writing. See the note above on why the order is the
        // property rather than an implementation detail.
        verify_bytes(artifact, &bytes)?;

        let path = dir.join(&artifact.file_name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| FetchError::Write {
                path: path.clone(),
                source,
            })?;
        }
        std::fs::write(&path, &bytes).map_err(|source| FetchError::Write {
            path: path.clone(),
            source,
        })?;
        Ok(path)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// The SHA-256 of the empty string, from the standard test vectors.
    const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    /// The hash function agrees with a published vector.
    ///
    /// Checked against a known constant rather than against itself, because a
    /// self-consistent hash implementation that is wrong would pass every
    /// round-trip test in this file.
    #[test]
    fn sha256_matches_the_published_empty_string_vector() {
        assert_eq!(sha256_hex(b""), EMPTY_SHA256);
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    /// ★ A mismatch is refused, and the message carries BOTH digests.
    ///
    /// Both, because the operator's next question is always "is my pin stale or
    /// is my download bad?", and one digest cannot answer it.
    #[test]
    fn a_hash_mismatch_is_refused_and_names_both_digests() {
        let art = PinnedArtifact::new("https://example.invalid/x", EMPTY_SHA256, "x.bin");
        let err = verify_bytes(&art, b"not empty").expect_err("must refuse");
        let msg = err.to_string();
        assert!(msg.contains(EMPTY_SHA256), "expected digest: {msg}");
        assert!(
            msg.contains(&sha256_hex(b"not empty")),
            "actual digest: {msg}"
        );
        assert!(msg.contains("nothing was written"), "must say so: {msg}");
    }

    /// A matching digest verifies.
    #[test]
    fn a_matching_digest_verifies() {
        let art = PinnedArtifact::new("https://example.invalid/x", sha256_hex(b"payload"), "x.bin");
        assert!(verify_bytes(&art, b"payload").is_ok());
    }

    /// An uppercase pinned digest is not a spurious mismatch.
    ///
    /// Checksum tools print uppercase as often as lowercase, and a mismatch
    /// caused purely by case would look identical to a tampered file — sending
    /// someone to hunt for a supply-chain attack that is really a `to_lower`.
    #[test]
    fn an_uppercase_pinned_digest_still_matches() {
        let art = PinnedArtifact::new(
            "https://example.invalid/x",
            EMPTY_SHA256.to_ascii_uppercase(),
            "x.bin",
        );
        assert!(verify_bytes(&art, b"").is_ok());
    }

    /// ★ With the feature stripped, the entry point REFUSES BY NAME.
    ///
    /// Rule 2 of the strippable-capability convention. Compiled only in that
    /// configuration, so it is the `--no-default-features` CI job that runs it.
    #[cfg(not(feature = "download"))]
    #[test]
    fn a_stripped_build_refuses_by_name_rather_than_failing_vaguely() {
        let art = PinnedArtifact::new("https://example.invalid/x", EMPTY_SHA256, "x.bin");
        let err = fetch_verified(&art, Path::new(".")).expect_err("must refuse");
        assert!(matches!(err, FetchError::FeatureUnsupported { .. }));
        let msg = err.to_string();
        assert!(msg.contains("x.bin"), "must name the file: {msg}");
        assert!(
            msg.contains("download") && msg.contains("manually"),
            "must say what to do instead: {msg}"
        );
    }

    /// ★ A non-HTTPS URL is refused before any request is made.
    ///
    /// Compiled only WITH the feature, since the stripped build refuses
    /// earlier for a different (also correct) reason.
    #[cfg(feature = "download")]
    #[test]
    fn a_plain_http_url_is_refused_without_touching_the_network() {
        let art = PinnedArtifact::new("http://example.invalid/x", EMPTY_SHA256, "x.bin");
        let err = fetch_verified(&art, Path::new(".")).expect_err("must refuse");
        assert!(
            matches!(err, FetchError::InsecureUrl { .. }),
            "expected InsecureUrl, got {err:?}"
        );
    }

    /// Verification is available with the fetcher stripped.
    ///
    /// The reason `verify_bytes` is not behind the feature: an operator who
    /// obtained a file by hand can still check it against the manifest.
    #[test]
    fn verification_works_regardless_of_the_download_feature() {
        let art = PinnedArtifact::new("https://example.invalid/x", sha256_hex(b"z"), "x.bin");
        assert!(verify_bytes(&art, b"z").is_ok());
    }
}
