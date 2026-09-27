//! Fuzz target: RFC 3161 `TimeStampResp` acceptance (`Pass 10.11`) over
//! arbitrary bytes — the one input to `EditSession::sign_with_timestamp`
//! that arrives from a network peer.
//!
//! The first two bytes (big-endian) give the response's length; the
//! remainder is a CMS the accepted token is embedded into. That
//! drives the `PKIStatusInfo`/`failInfo` reader, the `SignedData` and
//! `TSTInfo` walkers, the imprint/nonce comparison, the TSA certificate's
//! EKU check, the token's signature verification on attacker-chosen keys,
//! and the `SignerInfo` re-wrap.
//!
//! Invariant: for ANY input the call returns and never panics, aborts or
//! loops.
//!
//! Seeds: `fuzz/corpus/timestamp_response/seed_*`, an `openssl ts -reply`
//! answer to `fuzz_request_der()` over the synthetic TSA key.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::sign::timestamp::fuzz_accept_and_embed;

fuzz_target!(|data: &[u8]| {
    let [hi, lo, rest @ ..] = data else {
        return;
    };
    let at = usize::from(u16::from_be_bytes([*hi, *lo])).min(rest.len());
    let (response, cms) = rest.split_at(at);
    let _ = fuzz_accept_and_embed(response, cms);
});
