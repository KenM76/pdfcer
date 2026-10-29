//! Fuzz target: the IPP response decoder (`pdfcer_print::ipp`) over
//! arbitrary bytes — what a network printer, or anything answering on its
//! address, sends back.
//!
//! Invariants: no panic, abort or loop; a decoded response holds at most
//! `MAX_ATTRIBUTES` named attributes; and every accessor the CLI calls on a
//! response (`unsupported`, `job_id`, `status_message`, `printer_strings`)
//! is total on whatever decodes.
//!
//! Seeds: `fuzz/corpus/ipp_response/seed_*.ipp` — encoded messages built by
//! the codec's own encoder.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_print::ipp;

fuzz_target!(|data: &[u8]| {
    if let Ok(r) = ipp::parse_response(data) {
        let named: usize = r.groups.iter().map(|g| g.attributes.len()).sum();
        assert!(named <= ipp::MAX_ATTRIBUTES);
        let _ = (r.unsupported(), r.job_id(), r.status_message());
        let _ = r.printer_strings("document-format-supported");
        let _ = ipp::status_name(r.status);
    }
});
