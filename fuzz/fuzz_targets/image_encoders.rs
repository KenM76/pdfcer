//! Fuzz target: pdfcer's own filter encoders. Every encoder's output must
//! decode back to its input. Encoders see only pdfcer's pixels, but any
//! byte string is a valid input, so the identity must hold for all of them.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::filters::runlength;

fuzz_target!(|data: &[u8]| {
    let encoded = runlength::encode(data);
    assert!(encoded.len() <= data.len() + data.len() / 128 + 2);
    assert_eq!(runlength::decode(&encoded).ok().as_deref(), Some(data));
});
