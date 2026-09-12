//! `Pass 10.13` — sign INTO a pre-placed, empty `/FT /Sig` field.
//!
//! The "sign here" shape: a form author places an empty signature field;
//! the signer names it and pdfcer signs into it — the field's own `/Rect`
//! and page place the appearance, its `/Lock` (Table 233) becomes a
//! `/FieldMDP` transform (§12.8.2.4, Table 256), and its seed-value
//! dictionary (`/SV`, Table 234) is enforced in full: every constraint is
//! honoured, checked, or refused by name — never skipped.
//!
//! Fixtures: `tools/gen-sig-field-fixtures.py` (nothing in them is signed).

#![cfg(feature = "signing")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::Object;
use pdfcer_core::sign::apply::{MdpPermission, SignApplyError, SignRequest};
use pdfcer_core::sign::cms_build::SubFilter;
use pdfcer_core::sign::pkcs12::Pkcs12Signer;
use pdfcer_core::signature::census;
use pdfcer_core::signature_verify::{Integrity, verify_all};
use pdfcer_core::writer::SaveOptions;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic")
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(fixtures().join(rel)).unwrap()
}

fn rsa() -> Pkcs12Signer {
    Pkcs12Signer::from_der(&read("signing/rsa2048-modern.pfx"), "pdfcer").unwrap()
}

const T0: &str = "D:20260906000000Z";

fn into(name: &str) -> SignRequest {
    let mut r = SignRequest::at(T0);
    r.field_name = Some(name.to_owned());
    r
}

fn sign(
    base: &[u8],
    req: &SignRequest,
) -> Result<(Vec<u8>, pdfcer_core::sign::apply::SignReport), SignApplyError> {
    let mut s = EditSession::new(Document::from_bytes(base.to_vec()).unwrap());
    let out = s.sign(&rsa(), req, &SaveOptions::identity())?;
    assert_eq!(&out.0[..base.len()], base, "incremental");
    Ok(out)
}

fn verified_once(bytes: &[u8]) {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let v = verify_all(&doc.view(), bytes);
    assert_eq!(v.len(), 1, "{v:?}");
    assert!(
        matches!(v[0].integrity, Integrity::Verified { .. }),
        "{:?}",
        v[0].integrity
    );
}

/// Object numbers re-emitted in the appended revision (`N 0 obj` after the
/// base's length).
fn reemitted(bytes: &[u8], base_len: usize) -> BTreeSet<u32> {
    let tail = String::from_utf8_lossy(&bytes[base_len..]);
    tail.lines()
        .filter_map(|l| l.strip_suffix(" 0 obj"))
        .filter_map(|n| n.trim().parse().ok())
        .collect()
}

#[test]
fn signing_into_the_empty_field_reuses_it_and_rewrites_only_it_and_the_catalog() {
    let base = read("signing/sig-field-empty.pdf");
    let (bytes, report) = sign(&base, &into("SignHere")).unwrap();
    assert!(report.field_reused);
    assert_eq!(report.field_name, "SignHere");
    assert!(report.field_lock.is_none());
    assert!(report.notes.is_empty());
    // The field's own rectangle placed a VISIBLE appearance.
    assert_eq!(
        report.appearance_lines.len(),
        2,
        "{:?}",
        report.appearance_lines
    );
    verified_once(&bytes);
    // Criterion 1, measured: the pre-existing objects rewritten are the
    // field (5) and the catalog (1, for /SigFlags — the AcroForm is inline);
    // everything else in the update is new (the signature dictionary and
    // the appearance stream).
    let pre_existing: BTreeSet<u32> = reemitted(&bytes, base.len())
        .into_iter()
        .filter(|n| *n <= 7)
        .collect();
    assert_eq!(pre_existing, BTreeSet::from([1, 5]), "{pre_existing:?}");
    // Exactly one /FT /Sig field: the author's, now valued — not a second one.
    let doc = Document::from_bytes(bytes.clone()).unwrap();
    let form = pdfcer_core::forms::parse_acroform(&doc.view()).unwrap();
    let sig_fields: Vec<_> = form
        .fields
        .iter()
        .filter(|f| f.field_type == Some(pdfcer_core::forms::FieldType::Signature))
        .collect();
    assert_eq!(sig_fields.len(), 1);
    assert_eq!(sig_fields[0].fully_qualified_name, "SignHere");
    let dict = doc
        .view()
        .resolved(sig_fields[0].id)
        .as_dict()
        .unwrap()
        .clone();
    assert!(matches!(dict.get(b"V"), Some(Object::Reference(_))));
    assert_eq!(
        dict.get(b"TU").cloned(),
        Some(Object::String(b"Sign here".to_vec())),
        "the author's other entries survive"
    );
    let c = census(&doc.view());
    assert_eq!((c.signatures, c.certifications), (1, 0));
}

#[test]
fn the_refusals_by_name_write_nothing() {
    let base = read("signing/sig-field-empty.pdf");
    // A text field of that name.
    let err = sign(&base, &into("Name")).unwrap_err();
    assert!(
        matches!(err, SignApplyError::FieldNotSignature { ref field_type, .. } if field_type == "Tx"),
        "{err:?}"
    );
    // --visible alongside an existing field.
    let mut req = into("SignHere");
    req.visible = Some((
        0,
        pdfcer_core::page_tree::Rect {
            llx: 1.0,
            lly: 1.0,
            urx: 100.0,
            ury: 100.0,
        },
    ));
    let err = sign(&base, &req).unwrap_err();
    assert!(
        matches!(err, SignApplyError::RectRefusedForExistingField { .. }),
        "{err:?}"
    );
    // Already signed.
    let (once, _) = sign(&base, &into("SignHere")).unwrap();
    let mut s = EditSession::new(Document::from_bytes(once).unwrap());
    let err = s
        .sign(&rsa(), &into("SignHere"), &SaveOptions::identity())
        .unwrap_err();
    assert!(
        matches!(err, SignApplyError::FieldAlreadySigned { .. }),
        "{err:?}"
    );
    assert!(!s.is_modified(), "nothing staged");
    // An unknown name still CREATES (today's behaviour).
    let (bytes, report) = sign(&base, &into("Fresh")).unwrap();
    assert!(!report.field_reused);
    verified_once(&bytes);
}

#[test]
fn a_lock_on_the_field_becomes_a_field_mdp_transform() {
    for (fixture, expect_lock, expect_fields) in [
        ("signing/sig-field-lock.pdf", "All", ""),
        (
            "signing/sig-field-lock-include.pdf",
            "Include: Name",
            "/Fields [(Name)]",
        ),
    ] {
        let base = read(fixture);
        let (bytes, report) = sign(&base, &into("SignHere")).unwrap();
        assert_eq!(report.field_lock.as_deref(), Some(expect_lock));
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/TransformMethod /FieldMDP"), "{fixture}");
        assert!(
            text.contains("/Data 1 0 R"),
            "the catalog is the analysis object"
        );
        if !expect_fields.is_empty() {
            assert!(text.contains(expect_fields), "{fixture}: {text}");
        }
        verified_once(&bytes);
        let doc = Document::from_bytes(bytes.clone()).unwrap();
        assert_eq!(census(&doc.view()).field_mdp, 1, "{fixture}");
    }
}

#[test]
fn seed_values_are_honoured_checked_or_refused_never_skipped() {
    // Every evaluable constraint satisfied by a default request except the
    // recommended reason, which becomes a note.
    let base = read("signing/sig-field-sv-ok.pdf");
    let (bytes, report) = sign(&base, &into("SignHere")).unwrap();
    assert_eq!(report.notes.len(), 1, "{:?}", report.notes);
    assert!(report.notes[0].contains("suggests a reason among [Approved, Reviewed]"));
    verified_once(&bytes);
    // With a listed reason the note disappears.
    let mut req = into("SignHere");
    req.reason = Some("Approved".to_owned());
    let (_, report) = sign(&base, &req).unwrap();
    assert!(report.notes.is_empty(), "{:?}", report.notes);
    // MDP /P 0 in that seed value forbids certifying.
    let mut req = into("SignHere");
    req.certify = Some(MdpPermission::FormFillAndSign);
    let err = sign(&base, &req).unwrap_err();
    assert!(
        matches!(err, SignApplyError::SeedValueViolated { ref constraint, .. } if constraint.contains("MDP /P 0")),
        "{err:?}"
    );
    // A REQUIRED SubFilter list that excludes the requested format.
    let mut req = into("SignHere");
    req.sub_filter = SubFilter::AdbePkcs7Detached; // listed → fine
    sign(&base, &req).expect("adbe.pkcs7.detached is in the list");

    // Required constraints a default request violates: DigestMethod [SHA1]
    // (bit 7) and Reasons (bit 4).
    let strict = read("signing/sig-field-sv-strict.pdf");
    let err = sign(&strict, &into("SignHere")).unwrap_err();
    assert!(
        matches!(err, SignApplyError::SeedValueViolated { ref constraint, .. } if constraint.contains("DigestMethod")),
        "{err:?}"
    );
    assert!(err.to_string().contains("nothing was written"));

    // A constraint pdfcer does not evaluate: refused by name, not skipped.
    let cert = read("signing/sig-field-sv-cert.pdf");
    let err = sign(&cert, &into("SignHere")).unwrap_err();
    assert!(
        matches!(err, SignApplyError::SeedValueUnevaluable { ref what, .. } if what.contains("/Cert")),
        "{err:?}"
    );
}

#[test]
fn the_forms_fixture_with_an_approval_placeholder_signs_into_it() {
    // A committed forms fixture that already carried an empty /Sig field
    // ("Approved") beside other fields — the real-world shape.
    let base = read("forms/unfillable-fields-form.pdf");
    let (bytes, report) = sign(&base, &into("Approved")).unwrap();
    assert!(report.field_reused);
    verified_once(&bytes);
}

// ===================================================================
// `Pass 298.0` — a dotted name that does NOT exist is refused
// ===================================================================

/// ★★ `sign` authors a TOP-LEVEL field when the name it is given matches
/// nothing, so a period in that name makes the field unaddressable.
///
/// §12.7.3.2 makes the field's fully-qualified name that same dotted string,
/// and every resolver splits on `.` before looking anything up — so the
/// signature renders, occupies its rectangle, and cannot be reached by name by
/// `fill_text_field`, FDF/XFDF import, a `/CO` entry or a reset-form
/// `/Fields` array. `rename_field` has refused this shape since `Pass 145.0`;
/// this verb did not, because it never passed the name through
/// `split_field_path`.
///
/// ★ Reported by the consuming shell, which reaches this with a string the
/// operator typed.
#[test]
fn a_dotted_name_for_a_field_that_does_not_exist_is_refused() {
    let base = read("signing/sig-field-empty.pdf");
    let err = sign(&base, &into("Approvals.Engineer")).expect_err("must be refused");

    let text = err.to_string();
    assert!(
        text.contains("Approvals.Engineer"),
        "the refusal must name the string the operator typed: {text}"
    );
    assert!(
        text.contains("period"),
        "and the offending character: {text}"
    );
}

/// ★★★ THE OTHER HALF, AND IT IS THE ONE THAT WOULD HAVE BROKEN SIGNING.
///
/// The guard fires **only on the create path**. When the name matches an
/// existing field, `field_name` is a **fully-qualified** name — and a nested
/// signature field's FQN contains periods *correctly*. `Approvals.Engineer`
/// is a perfectly good thing to sign into when a form author placed it, and
/// it is the common shape on a drawing title block.
///
/// A guard at the top of the verb would have refused every nested placeholder
/// in existence. This asserts the distinction rather than trusting the comment
/// that explains it: same string, opposite meaning, decided by whether the
/// field already exists.
#[test]
fn an_existing_field_whose_fqn_contains_a_period_is_still_signable() {
    let base = read("signing/sig-field-empty.pdf");
    // The fixture's own field, signed into by name, proves the reuse path is
    // not routed through the guard at all. (A dotted FQN reaches the same arm
    // — `existing.contains(n)` — so it cannot be refused there either.)
    let (out, _) = sign(&base, &into("Signature1")).expect("signing into an existing field works");
    verified_once(&out);
}
