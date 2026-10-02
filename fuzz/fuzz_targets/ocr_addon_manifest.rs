//! Fuzz target: OCR model add-on manifests
//! (`pdfcer_core::ocr::addon_manifest`).
//!
//! A manifest arrives in a folder someone dropped in, so it is untrusted.
//! Invariant: `parse_manifest_bytes` never panics, and an accepted manifest
//! satisfies the documented limits: a name that passes `checked_name`, every
//! file path relative with no `..` and no backslash, and no file listed twice.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::ocr::addon_manifest::{MAX_MANIFEST_BYTES, checked_name, parse_manifest_bytes};

fuzz_target!(|data: &[u8]| {
    let Ok(m) = parse_manifest_bytes(data) else {
        return;
    };
    assert!(data.len() <= MAX_MANIFEST_BYTES);
    assert_eq!(checked_name(&m.name).as_deref(), Ok(m.name.as_str()));
    assert!(!m.engine.is_empty());
    let mut seen = std::collections::HashSet::new();
    for f in &m.files {
        assert!(!f.file.starts_with('/') && !f.file.contains('\\') && !f.file.contains(':'));
        assert!(
            f.file
                .split('/')
                .all(|c| !c.is_empty() && c != "." && c != "..")
        );
        assert!(seen.insert(f.file.as_str()));
    }
});
