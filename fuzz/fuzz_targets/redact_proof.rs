//! Fuzz target: the redaction absence proof
//! (`pdfcer_core::redact::verify_absence`) over untrusted output bytes.
//!
//! The first input byte splits the rest into a needle and the document, so
//! needles of every length (including below the verifiable floor) meet
//! parseable, unparseable and hostile-stream inputs. It must never panic,
//! and an unparsable document must never read as verified.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::redact::{AbsenceVerdict, verify_absence};

fuzz_target!(|data: &[u8]| {
    let Some((&split, rest)) = data.split_first() else {
        return;
    };
    let at = usize::from(split).min(rest.len());
    let (needle, doc) = rest.split_at(at);
    let needles = vec![String::from_utf8_lossy(needle).into_owned()];
    let proof = verify_absence(doc, &needles);
    if proof.output_unparsed {
        assert_ne!(proof.verdict(), AbsenceVerdict::Verified);
    }
});
