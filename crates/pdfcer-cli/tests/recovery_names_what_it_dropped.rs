//! # The CLI names the objects recovery could not keep (owed item 33)
//!
//! `Pass 302.0` gave `RecoveryReport` an `objects_dropped` list so recovery
//! would stop returning a shorter document with no explanation. **It wired
//! nothing to it.** Every other field on that struct is printed by
//! `disclose_recovery`; this one was not, so from a terminal the document was
//! still silently shorter — which is the half of rule 4 that actually bites.
//! The struct's own doc comment meanwhile claimed the CLI surfaces every field
//! and that "none is rounded away".
//!
//! That is `R245`'s shape: a disclosure added to one member of a family and
//! not shipped until a test walks the whole family. This file is that test for
//! the one field it missed.
//!
//! ## Why an out-of-process test
//!
//! The unit tests in `recover.rs` prove the REPORT carries the loss. They
//! cannot prove an operator ever sees it — that is a property of the binary's
//! stderr, and only running the binary can observe it. The distinction is the
//! entire content of owed item 33.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// A file whose `startxref` is absent (forcing recovery) and which carries one
/// object header the parser cannot finish.
///
/// Written byte by byte here rather than committed as a fixture: it is four
/// lines, its defect is the point, and a reader can see exactly what is wrong
/// with it without opening another file. Object 3's body is `<< /Type` and
/// then the file ends.
fn damaged() -> PathBuf {
    let dir = std::env::temp_dir().join("pdfcer-recovery-drop-tests");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let p = dir.join("one-unparseable-object.pdf");
    let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [4 0 R] >>\nendobj\n\
4 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>\nendobj\n\
3 0 obj\n<< /Type \n";
    std::fs::write(&p, bytes).expect("write the damaged fixture");
    p
}

/// ★★★ THE OPERATOR IS TOLD WHICH OBJECT WENT, AND WHY.
///
/// Asserting on the NUMBER, not just on the count. The complaint owed item 18
/// came from was never that a tally was wrong — it was that a human holding a
/// file with a missing page could not learn WHICH object went or WHY. A test
/// that only checked "some note was printed" would pass on `dropped: 1`, which
/// answers neither question.
#[test]
fn the_cli_names_the_dropped_object_on_stderr() {
    let out = Command::new(BIN)
        .args(["inspect", damaged().to_str().unwrap()])
        .output()
        .expect("run pdfcer inspect");
    let err = String::from_utf8_lossy(&out.stderr);

    assert!(
        err.contains("NOT kept"),
        "recovery dropped an object and said nothing on stderr:\n{err}"
    );
    assert!(
        err.contains(": 3."),
        "the disclosure must name object 3 specifically, not merely count it:\n{err}"
    );
    assert!(
        err.contains("did not parse"),
        "and must say WHY, so a false positive reads differently from a real \
         loss:\n{err}"
    );
}

/// ★ AND A CLEAN RECOVERY SAYS NOTHING — so the note above is a signal rather
/// than boilerplate every recovered file carries.
///
/// A disclosure printed on every recovery trains its reader to skip it, which
/// is how a real loss goes unread. This is the half of the pair that keeps the
/// other half meaningful.
#[test]
fn a_recovery_that_kept_everything_prints_no_drop_note() {
    let dir = std::env::temp_dir().join("pdfcer-recovery-drop-tests");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let p = dir.join("nothing-dropped.pdf");
    let bytes: &[u8] = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [4 0 R] >>\nendobj\n\
4 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>\nendobj\n";
    std::fs::write(&p, bytes).expect("write");

    let out = Command::new(BIN)
        .args(["inspect", p.to_str().unwrap()])
        .output()
        .expect("run pdfcer inspect");
    let err = String::from_utf8_lossy(&out.stderr);

    assert!(
        err.contains("recovery"),
        "premise: this file must still recover, or the test asserts nothing \
         about recovery's output:\n{err}"
    );
    assert!(
        !err.contains("NOT kept"),
        "nothing was dropped, so nothing may be reported as dropped:\n{err}"
    );
}
