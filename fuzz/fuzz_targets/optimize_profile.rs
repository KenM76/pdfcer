//! Fuzz target: optimize profiles (`pdfcer_core::optimize::profile`).
//!
//! Profiles are user-supplied files, so the parser must never panic, an
//! `extends` chain resolving to itself must stop at the cycle or depth guard,
//! and every accepted profile must survive `to_profile` -> `parse` unchanged.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::optimize::profile;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let _ = profile::parse_rule(text);
    let _ = profile::parse_object(text);
    let own = text.to_owned();
    let mut resolve = |_: &str| Ok(own.clone());
    if let Ok(settings) = profile::parse("fuzz", text, &mut resolve) {
        let written = profile::to_profile(&settings);
        let again = profile::parse("fuzz", &written, &mut |n| Err(n.to_owned()));
        assert_eq!(again.as_ref(), Ok(&settings), "{written}");
    }
});
