//! Fuzz target: the ONNX opset upgrader (`pdfcer_core::ocr::onnx_upgrade`).
//!
//! Model files are operator-supplied, so their bytes are untrusted.
//! Invariant: `upgrade` returns `Ok` or `UpgradeError` and never panics; a
//! rewritten model is itself well-formed protobuf (upgrading it again is not
//! `Malformed`); an unrewritten one is the input byte-for-byte.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::ocr::onnx_upgrade::{UpgradeError, upgrade};

fuzz_target!(|data: &[u8]| {
    let Ok(up) = upgrade(data.to_vec()) else {
        return;
    };
    if up.rewritten == 0 {
        assert_eq!(up.bytes, data);
        return;
    }
    assert!(!matches!(
        upgrade(up.bytes),
        Err(UpgradeError::Malformed(_))
    ));
});
