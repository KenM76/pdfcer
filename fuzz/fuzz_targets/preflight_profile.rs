//! Fuzz target: preflight profile JSON (`pdfcer_core::preflight::Profile`).
//!
//! Profiles are user-supplied files: the parser must never panic, and every
//! accepted profile must survive `to_json` -> `from_json` unchanged.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::preflight::Profile;

fuzz_target!(|data: &[u8]| {
    if let Ok(profile) = Profile::from_json(data) {
        let written = profile.to_json();
        let again = Profile::from_json(written.as_bytes());
        assert_eq!(again.as_ref(), Ok(&profile), "{written}");
    }
});
