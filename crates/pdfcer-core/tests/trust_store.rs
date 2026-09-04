//! `Pass 10.2` — reading a REAL installed Acrobat/Reader trust store.
//!
//! The unit tests in `crate::trust_store` prove the walk and the source/`/Trust`
//! extraction against a synthetic address book. This integration test is the
//! other half: it opens the ACTUAL `addressbook.acrodata` on this machine and
//! confirms pdfcer's COS reuse (the `%PPKLITE-` header seam + the Pass 10.1
//! X.509 decoder) reads Adobe's real, downloaded AATL/EUTL anchor set.
//!
//! It is **environment-gated**: the file exists only on a machine with Acrobat/
//! Reader installed and refreshed. When absent, the test prints a note and
//! passes — exactly like the `fixtures/external/` corpus tests — so CI (which
//! has no Acrobat) stays green while a developer machine gets the real check.

use std::path::PathBuf;

use pdfcer_core::trust_store::{self, SourceFilter};

/// The default Windows location of Acrobat/Reader's trust store, across the
/// track directories a real install may use.
fn candidate_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(appdata) = std::env::var("APPDATA") {
        for track in ["DC", "2020", "2017", "11.0"] {
            out.push(
                PathBuf::from(&appdata)
                    .join("Adobe")
                    .join("Acrobat")
                    .join(track)
                    .join("Security")
                    .join("addressbook.acrodata"),
            );
        }
    }
    out
}

#[test]
fn reads_the_installed_acrobat_trust_store() {
    let Some(path) = candidate_paths().into_iter().find(|p| p.exists()) else {
        eprintln!(
            "trust_store: no installed Acrobat/Reader addressbook.acrodata found \
             (%APPDATA%\\Adobe\\Acrobat\\<track>\\Security\\). Skipping the \
             real-store check — this is expected on CI and any machine without \
             Acrobat."
        );
        return;
    };

    let set = trust_store::load_from_path(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

    // A refreshed Acrobat has thousands of anchors; be tolerant of the exact
    // count (it tracks whatever Acrobat downloaded) but insist it is non-trivial.
    assert!(
        set.len() > 50,
        "a real trust store should carry many anchors, got {}",
        set.len()
    );

    let c = set.counts();
    // AATL and EUTL are the two downloaded lists; both should be present on a
    // real, refreshed install. (The 2024 specimen this feature was built from
    // held 1567 EUTL + 199 AATL + 1 ADBE.)
    assert!(c.aatl > 0, "expected some AATL anchors, got {c:?}");
    assert!(c.eutl > 0, "expected some EUTL anchors, got {c:?}");

    // Every anchor in the set decoded to a real X.509 (undecodable ones are
    // excluded by construction): spot-check that subjects and serials are real.
    assert!(
        set.anchors.iter().all(|a| !a.serial_hex.is_empty()),
        "every anchor has a serial"
    );
    assert!(
        set.anchors.iter().any(|a| !a.subject.is_empty()),
        "at least one anchor names a subject"
    );

    // The source filter narrows to exactly the AATL set (the operator's 55.6%
    // concern — this is how a shell would offer "trust AATL only").
    let aatl = set.filter(SourceFilter::Aatl);
    assert_eq!(aatl.len(), c.aatl);

    eprintln!(
        "trust_store: read {} anchors from {} (AATL={} EUTL={} ADBE={} other={}, {} undecodable)",
        set.len(),
        path.display(),
        c.aatl,
        c.eutl,
        c.adbe,
        c.other,
        set.undecodable,
    );
}
