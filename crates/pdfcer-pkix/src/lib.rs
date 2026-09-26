//! pdfcer public-key infrastructure: DER/ASN.1 reading (`asn1`), CMS
//! `SignedData` and X.509 certificate parsing (`cms`, RFC 5652 / RFC 5280),
//! certificate-chain building and validation (`trust_chain`) and the trust
//! store (`trust_store`).
//!
//! Depends only on `pdfcer-model` (for its hash and public-key primitives).
//! `pdfcer-core` re-exports `trust_chain` and `trust_store` at their old
//! paths (`docs/ARCHITECTURE.md` §3). `asn1` and `cms` are workspace-internal:
//! `pdfcer-core`'s signing and verification call them, and they are not API.

// Parses untrusted DER, so a reachable panic is a denial-of-service bug.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

#[doc(hidden)] // workspace-internal: called by pdfcer-core, not API
pub mod asn1;
#[doc(hidden)] // workspace-internal: called by pdfcer-core, not API
pub mod cms;
pub mod trust_chain;
pub mod trust_store;
