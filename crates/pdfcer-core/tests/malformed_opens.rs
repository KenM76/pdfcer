//! # A file with errors opens, and pdfcer says what it decided
//!
//! ## The operator's ruling, verbatim
//!
//! > *"We should be making pdfcer so that it opens pdfs that have errors, and
//! > have a way that it manages those errors such that they aren't fatal, and
//! > if the user can intervene in a decision that should always be an option
//! > along with them not having to intervene."*
//!
//! Three obligations, and this file measures all three:
//!
//! 1. **Not fatal** — the document loads.
//! 2. **Intervention possible** — the decision is recorded with *both* values,
//!    and re-loading under the other policy takes the other one.
//! 3. **Intervention optional** — the default needs no argument and no
//!    attention.
//!
//! ## The file that forced it
//!
//! A real drawing (`A-726 BASKET ATTACHMENT_REV 5.pdf`, 46,709 bytes) that
//! Acrobat opens and pdfcer refused **twice over**: its catalog names
//! `/PageMode` twice with different values, and — one object later — its
//! `/Metadata` stream carries **no `/Length` at all**. Each defect on its own
//! cost the whole document.
//!
//! ## What the standard says, and why a policy is the honest shape
//!
//! §7.3.7 is a genuine `shall not` — *"Multiple entries in the same dictionary
//! shall not have the same key"*, identical in both editions — and it binds
//! the **file**. §2.1/§2.3 make conformance a property of files and writers;
//! clause 1 excludes validation methods outright. So ISO 32000 neither obliges
//! pdfcer to render such a file nor obliges it to refuse. `pdf-issues` #199
//! (open since 2022) says so directly: *"beyond the scope of ISO 32000."*
//!
//! Which value wins is therefore **pdfcer's choice to make and to disclose**.
//! Keep-last is chosen on observed behaviour — qpdf, pdf.js and pdfium all
//! keep the last occurrence and none refuses — not on an analogy to §7.5.6,
//! whose ordering the standard makes meaningful precisely where §7.3.7's
//! *"shall be ignored"*.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::{Document, LoadAnomaly, LoadOptions};
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::parser::DuplicateKeyPolicy;

/// A one-page PDF whose catalog names `/PageMode` twice, with different
/// values — the operator's file in miniature.
fn pdf_with_duplicate_page_mode() -> Vec<u8> {
    build(&[
        "<< /Type /Catalog /Pages 2 0 R /PageMode /UseOC /PageMode /UseOutlines >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << >> >>",
    ])
}

/// A one-page PDF whose stream object omits `/Length` entirely (§7.3.8.2
/// Table 5 makes it REQUIRED).
fn pdf_with_lengthless_stream() -> Vec<u8> {
    build(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << >> /Contents 4 0 R >>",
        "<< >>\nstream\n0 0 100 100 re f\nendstream",
    ])
}

fn build(bodies: &[&str]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

// ------------------------------------------------------- 1. it is not fatal

/// ★★★ THE OPERATOR'S FIRST OBLIGATION: the file opens.
#[test]
fn a_duplicate_key_no_longer_costs_the_document() {
    let doc = Document::from_bytes(pdf_with_duplicate_page_mode())
        .expect("a repeated key is a malformed ENTRY, not an unreadable file");
    assert!(doc.catalog().is_ok());
}

/// The same obligation for a second, unrelated malformation — because a file
/// that opens past the first defect and dies on the second has not been fixed.
///
/// ★ This is not hypothetical symmetry: the operator's file did exactly that.
/// The duplicate key was cleared, and the very next object — a `/Metadata`
/// stream with no `/Length` — refused it again.
#[test]
fn a_stream_with_no_length_no_longer_costs_the_document() {
    let doc = Document::from_bytes(pdf_with_lengthless_stream())
        .expect("the data extent is recoverable by scanning to `endstream`");
    assert!(doc.catalog().is_ok());
}

// -------------------------------------------- 2. the decision is recoverable

/// ★★ THE INTERVENTION IS REAL: the record names what was chosen AND what was
/// chosen between.
///
/// A count would satisfy "pdfcer disclosed something". Only the pair lets a
/// shell offer the operator the other value, which is what the ruling asks
/// for.
#[test]
fn the_decision_records_both_values_not_just_a_count() {
    let doc = Document::from_bytes(pdf_with_duplicate_page_mode()).unwrap();
    let anomalies = doc.load_anomalies();
    assert_eq!(anomalies.len(), 1, "{anomalies:?}");
    let LoadAnomaly::DuplicateDictKey {
        object,
        key,
        kept,
        discarded,
    } = &anomalies[0]
    else {
        panic!("expected a duplicate-key anomaly, got {anomalies:?}");
    };
    assert_eq!(*object, Some(ObjId::new(1, 0)));
    assert_eq!(key.as_slice(), b"PageMode");
    assert_eq!(kept, &Object::Name(b"UseOutlines".into()));
    assert_eq!(
        discarded,
        &Object::Name(b"UseOC".into()),
        "the discarded value is the one a shell offers as the alternative"
    );
}

/// ★★ AND TAKING THE ALTERNATIVE WORKS. Without this the record would be a
/// description of a choice nobody can actually make.
#[test]
fn re_loading_under_keep_first_takes_the_other_value() {
    let bytes = pdf_with_duplicate_page_mode();
    let last = Document::from_bytes(bytes.clone()).unwrap();
    let first = Document::from_bytes_with_options(
        bytes,
        None,
        LoadOptions::new().with_duplicate_keys(DuplicateKeyPolicy::KeepFirst),
    )
    .expect("the other policy still opens the file");

    let mode = |d: &Document| -> Vec<u8> {
        let Object::Name(n) = d.catalog().unwrap().get(b"PageMode").unwrap() else {
            panic!("not a name")
        };
        n.as_bytes().to_vec()
    };
    assert_eq!(mode(&last), b"UseOutlines");
    assert_eq!(mode(&first), b"UseOC");
}

/// The `/Length` recovery is recorded too — even though there is no second
/// reading to offer.
///
/// ★ The record's purpose differs by kind, and saying so is the point: for a
/// duplicate key it enables a choice, and here it tells the operator the file
/// is damaged. A disclosure that only appeared when a choice existed would let
/// the more serious defect pass unmentioned.
#[test]
fn a_recovered_stream_length_is_recorded_even_with_no_alternative() {
    let doc = Document::from_bytes(pdf_with_lengthless_stream()).unwrap();
    let anomalies = doc.load_anomalies();
    assert!(
        anomalies
            .iter()
            .any(|a| matches!(a, LoadAnomaly::StreamLengthRecovered { .. })),
        "{anomalies:?}"
    );
}

// ------------------------------------------ 3. intervention stays OPTIONAL

/// A clean file records nothing — the control.
///
/// Without it, an implementation that reported an anomaly for every document
/// would pass every assertion above and make the disclosure worthless.
#[test]
fn a_clean_file_records_no_anomalies() {
    let doc = Document::from_bytes(build(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << >> >>",
    ]))
    .unwrap();
    assert!(doc.load_anomalies().is_empty());
}

/// The strict behaviour is still reachable, by name.
///
/// The pre-`Pass 283.0` answer is not deleted — a conformance checker, a fuzz
/// harness or a gate wants exactly it. What changed is which one a person
/// opening a document gets by default.
#[test]
fn strict_options_still_refuse_both_malformations() {
    assert!(
        Document::from_bytes_with_options(
            pdf_with_duplicate_page_mode(),
            None,
            LoadOptions::strict()
        )
        .is_err(),
        "strict must still refuse a duplicate key"
    );
    assert!(
        Document::from_bytes_with_options(
            pdf_with_lengthless_stream(),
            None,
            LoadOptions::strict()
        )
        .is_err(),
        "strict must still refuse a missing /Length"
    );
}

/// ★ The parser's OWN default is unchanged, and that is deliberate.
///
/// `LoadOptions` is tolerant; `DuplicateKeyPolicy::default()` is `Refuse`. Every
/// caller that constructs a `Parser` directly — fuzz targets, the recovery
/// confirmation pass — keeps the behaviour it was written against. The loader
/// opts in; the parser does not opt in for it.
#[test]
fn the_parsers_own_default_is_still_strict() {
    assert_eq!(DuplicateKeyPolicy::default(), DuplicateKeyPolicy::Refuse);
}

// ------------------------- 4. "all defects where it is possible to continue"

/// ★★★ ONE UNREADABLE OBJECT NO LONGER COSTS THE DOCUMENT.
///
/// The operator's second instruction: *"we should be doing this for all
/// defects where it is possible to continue and open the file."*
///
/// Object 4's body is garbage. §7.3.10 already says what a reference to an
/// object that is not there means — *"shall be treated as a reference to the
/// null object"*, and *"shall not be considered an error"* — so loading the
/// document without it produces a state the standard describes, rather than
/// one pdfcer invented. The page tree is intact and the file opens.
#[test]
fn one_unparseable_object_does_not_cost_the_document() {
    let doc = Document::from_bytes(build(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << >> >>",
        "<< /Broken (unterminated",
    ]))
    .expect("one bad object is one undefined object, not an unreadable file");

    assert!(doc.catalog().is_ok(), "the document is still usable");
    assert!(
        doc.get(ObjId::new(4, 0)).is_none(),
        "the unreadable object is ABSENT, which is exactly what §7.3.10 makes \
         a reference to it mean"
    );
    let anomalies = doc.load_anomalies();
    assert!(
        anomalies
            .iter()
            .any(|a| matches!(a, LoadAnomaly::ObjectUnreadable { object, .. }
                if *object == ObjId::new(4, 0))),
        "the operator is told WHICH object was lost: {anomalies:?}"
    );
}

/// The reason is carried, not just the object number.
///
/// ★ "Object 4 could not be read" is a fact; "object 4 could not be read
/// because …" is something an operator can act on — send the file back to its
/// producer, or decide the loss does not matter. A count would have been
/// neither.
#[test]
fn an_unreadable_object_records_why() {
    let doc = Document::from_bytes(build(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << >> >>",
        "<< /Broken (unterminated",
    ]))
    .unwrap();
    let LoadAnomaly::ObjectUnreadable { reason, .. } = doc
        .load_anomalies()
        .iter()
        .find(|a| matches!(a, LoadAnomaly::ObjectUnreadable { .. }))
        .expect("recorded")
    else {
        unreachable!()
    };
    assert!(!reason.is_empty(), "a reason with no words is a count");
}

/// And strict still refuses it — the pre-`Pass 283.0` behaviour, by name.
#[test]
fn strict_still_refuses_an_unreadable_object() {
    assert!(
        Document::from_bytes_with_options(
            build(&[
                "<< /Type /Catalog /Pages 2 0 R >>",
                "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << >> >>",
                "<< /Broken (unterminated",
            ]),
            None,
            LoadOptions::strict(),
        )
        .is_err()
    );
}

/// ★ THE LINE THAT IS STILL FATAL, and it is fatal for a reason that is not
/// strictness.
///
/// A file with no `/Root` has no document to show. Continuing would mean
/// pdfcer inventing a catalog, which is the one thing none of this Pass does —
/// every other decision here picks between things the FILE said, or applies a
/// reading the standard already prescribes.
///
/// Without this test, "open everything" could be read as a mandate to
/// fabricate, and the next person to widen the policy would have no recorded
/// boundary to stop at.
#[test]
fn a_file_with_no_catalog_is_still_refused() {
    let bytes = build(&["<< /NotACatalog true >>"]);
    // Strip the trailer's /Root so nothing names a catalog.
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let patched = text.replace("/Root 1 0 R", "            ");
    assert!(
        Document::from_bytes(patched.into_bytes()).is_err(),
        "there is nothing to open, and inventing a catalog is not continuing"
    );
}

// ------------------------------ 5. the intervention is reachable BY PATH too

/// ★★ THE INTERVENTION MUST BE REACHABLE THE WAY SHELLS ACTUALLY OPEN FILES.
///
/// `Pass 283.0` shipped the alternative reading on the **bytes** entry point
/// only, and every shell opens a **path** — the GUI's open-file action, the
/// CLI's path argument. Taking the other value therefore meant re-implementing
/// `Document::load`'s `std::fs::read` at the call site. That is the shape of a
/// guard present on one route and absent on its twin (**R245**), applied to an
/// affordance rather than a check: the route the intended caller uses did not
/// have it.
///
/// This test opens the SAME path twice and gets the two different readings, so
/// it fails if `load_with_options` ever silently degrades to `load`.
#[test]
fn the_other_reading_is_reachable_from_a_path() {
    let path = std::env::temp_dir().join("pdfcer-malformed-opens-by-path.pdf");
    std::fs::write(&path, pdf_with_duplicate_page_mode()).expect("fixture written");

    let mode = |d: &Document| -> Vec<u8> {
        let Object::Name(n) = d.catalog().unwrap().get(b"PageMode").unwrap() else {
            panic!("not a name")
        };
        n.as_bytes().to_vec()
    };

    let last = Document::load(&path).expect("the default posture opens it");
    assert_eq!(mode(&last), b"UseOutlines");
    assert!(!last.load_anomalies().is_empty(), "and says it decided");

    let first = Document::load_with_options(
        &path,
        None,
        LoadOptions::new().with_duplicate_keys(DuplicateKeyPolicy::KeepFirst),
    )
    .expect("and the same path opens under the other policy");
    assert_eq!(mode(&first), b"UseOC");

    let _ = std::fs::remove_file(&path);
}

/// And `strict()` reaches the path route as well — a conformance checker that
/// takes a filename is exactly the caller `strict()` was kept for, and it would
/// have had to read the bytes itself.
#[test]
fn strict_is_reachable_from_a_path() {
    let path = std::env::temp_dir().join("pdfcer-malformed-opens-by-path-strict.pdf");
    std::fs::write(&path, pdf_with_duplicate_page_mode()).expect("fixture written");

    assert!(
        Document::load_with_options(&path, None, LoadOptions::strict()).is_err(),
        "strict refuses the same file the default posture opens"
    );

    let _ = std::fs::remove_file(&path);
}
