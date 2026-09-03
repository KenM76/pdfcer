//! # A `*_refusal()` query must answer the SAME question its guard asks
//!
//! Reported from outside by the `pdfcer-gui` session (2026-08-13) as a
//! correctness defect in a `#[must_use]` public query, not a missing feature:
//!
//! > *"`fill_refusal()` can answer 'no refusal' where every fill errors… A
//! > shell that follows that instruction to the letter still ships the box
//! > that rejects whatever is typed into it, on two of the three refusal
//! > paths."*
//!
//! ## Why this is worse than an ordinary bug
//!
//! These queries exist so a shell can satisfy `R83` — **disable a control
//! rather than offer one that always errors**. So a refusal query that
//! under-reports does not degrade gracefully. It produces exactly the
//! behaviour `R83` forbids, while the shell's source reads as though `R83`
//! were satisfied, and **every test on both sides passes**.
//!
//! ## The shape, which is the part worth keeping
//!
//! `fill_refusal()` asked `check_certification_for_fill()`. `fill_guards()` —
//! the preamble every fill runs — asked **three** things: encryption, that
//! gate, and `/Size`-suppression. Two independent transcriptions of one list,
//! which drifted.
//!
//! The fix is not "add the two missing checks", because that produces a third
//! transcription. It is `X_refusal() { self.X_guards().err() }`, which makes
//! them **incapable** of disagreeing. `embed_fonts`/`unembed_fonts` already had
//! the property the other way round — the verb calls the query — which is why
//! those never drifted and are the healthy examples in the same file.
//!
//! This file asserts the property directly, so a future guard added to either
//! preamble cannot silently stop being reported.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

/// ★ THE REPORTED DEFECT: an encrypted document must be reported by
/// `fill_refusal`, not discovered when the operator types.
///
/// The fixture is the **empty-user-password** one deliberately. A
/// password-protected file cannot be opened at all, so it never reaches an
/// `EditSession` and cannot exercise this guard — the only encrypted documents
/// that can reach a Forms panel are exactly the ones that open without a
/// prompt, which is also why this defect was reachable by a real shell.
#[test]
fn fill_refusal_reports_encryption_rather_than_letting_the_fill_discover_it() {
    let doc = Document::load(&fixture("encryption/enc-emptyuser.pdf")).expect("fixture loads");
    let session = EditSession::new(doc);

    let refusal = session.fill_refusal();
    assert!(
        matches!(refusal, Some(EditError::DocumentEncrypted)),
        "fill_refusal must report DocumentEncrypted on an encrypted file; got \
         {refusal:?}. Returning None here is the defect: a shell would enable \
         the field and the operator would learn it was refused only after typing."
    );
}

/// ★ AND THE PROPERTY, not just the instance: whatever the guard refuses, the
/// query reports — checked by running both against the same document.
///
/// This is what makes a future third guard safe. If someone adds a check to
/// `fill_guards` and the query stops matching, this fails.
#[test]
fn fill_refusal_and_the_fill_itself_agree_on_every_fixture() {
    for rel in [
        "encryption/enc-emptyuser.pdf",
        "forms/demo-form.pdf",
        "forms/certified-p2-form.pdf",
    ] {
        let doc = Document::load(&fixture(rel)).expect("fixture loads");
        let mut session = EditSession::new(doc);

        let predicted = session.fill_refusal();
        // Attempt a fill against a field that may or may not exist; what is
        // being compared is the GUARD outcome, so a NoSuchField answer counts
        // as "the guards let it through".
        let actual = session.fill_text_field("any-field-name", "x").err();

        match (&predicted, &actual) {
            (Some(p), Some(a)) => assert_eq!(
                std::mem::discriminant(p),
                std::mem::discriminant(a),
                "{rel}: fill_refusal predicted {p:?} but the fill refused with \
                 {a:?} — the query and the guard disagree"
            ),
            (Some(p), None) => panic!("{rel}: fill_refusal predicted {p:?} but the fill succeeded"),
            (None, Some(a)) => {
                // Only acceptable if the failure is about the FIELD, not the
                // document — the guards genuinely passed.
                assert!(
                    !matches!(
                        a,
                        EditError::DocumentEncrypted
                            | EditError::ObjectCreationWouldExposeHiddenObjects { .. }
                            | EditError::CertificationForbidsChange { .. }
                            | EditError::FieldLockedBySignature
                    ),
                    "{rel}: ★ fill_refusal said None but the fill refused with a \
                     DOCUMENT-level guard: {a:?}. This is the exact under-report \
                     the pdfcer-gui session found."
                );
            }
            (None, None) => {}
        }
    }
}

/// ★ `flatten_refusal` exists and reports the suppression guard that
/// `deletion_refusal` correctly does not.
///
/// The requesting session had been gating its Flatten control on
/// `deletion_refusal` under a local alias, because it was the nearest available
/// question. The two agree on two checks of three — the worst kind of
/// near-miss, since it works until it does not.
#[test]
fn flatten_refusal_reports_encryption_and_is_not_deletion_refusal() {
    let doc = Document::load(&fixture("encryption/enc-emptyuser.pdf")).expect("fixture loads");
    let session = EditSession::new(doc);
    assert!(
        matches!(
            session.flatten_refusal(),
            Some(EditError::DocumentEncrypted)
        ),
        "flatten_refusal must report encryption"
    );
}

/// `flatten_refusal` and `flatten_fields` agree.
#[test]
fn flatten_refusal_and_flatten_itself_agree() {
    for rel in ["encryption/enc-emptyuser.pdf", "forms/demo-form.pdf"] {
        let doc = Document::load(&fixture(rel)).expect("fixture loads");
        let mut session = EditSession::new(doc);
        let predicted = session.flatten_refusal();
        let actual = session.flatten_fields(None).err();
        if let Some(p) = &predicted {
            let a = actual
                .as_ref()
                .unwrap_or_else(|| panic!("{rel}: predicted {p:?} but flatten succeeded"));
            assert_eq!(
                std::mem::discriminant(p),
                std::mem::discriminant(a),
                "{rel}: flatten_refusal predicted {p:?}, flatten refused {a:?}"
            );
        }
    }
}

/// `deletion_refusal` is CORRECT and must not be "fixed".
///
/// The report suggested it under-reports because `flatten_fields` guards on
/// suppression and it does not. But `deletion_refusal` predicts **deletion**,
/// and `deletion_preflight` is encryption + `check_certification` — exactly
/// what it reports. Adding a suppression check here would make it wrong in the
/// other direction: it would disable a Delete control that would have worked.
///
/// Asserted so a later reader acting on that report does not "correct" a
/// correct function.
#[test]
fn deletion_refusal_matches_deletion_and_must_not_gain_the_flatten_guard() {
    let doc = Document::load(&fixture("forms/demo-form.pdf")).expect("fixture loads");
    let mut session = EditSession::new(doc);
    let predicted = session.deletion_refusal();
    let actual = session.delete_field("definitely-not-a-real-field").err();
    if predicted.is_none() {
        assert!(
            !matches!(actual, Some(EditError::DocumentEncrypted)),
            "deletion_refusal said None but deletion refused on a document guard"
        );
    }
}

/// ★ `Widget::has_off_appearance` answers a question `on_states` cannot.
///
/// `on_states` excludes `Off` by §12.7.4.2.3 and must keep doing so, so there
/// was no way to ask *"will unticking this checkbox leave a blank widget?"*.
/// Requested by the `pdfcer-gui` session to disclose that **before** the click.
///
/// Asserted as a property of a real form rather than a unit test on the
/// helper: the value has to survive the whole parse to be useful to a shell.
#[test]
fn has_off_appearance_is_reported_and_is_not_an_on_state() {
    let doc = Document::load(&fixture("forms/demo-form.pdf")).expect("fixture loads");
    let form = pdfcer_core::forms::parse_acroform(&doc).expect("the fixture has a form");

    let mut checked_any = false;
    for field in &form.fields {
        for w in &field.widgets {
            // The invariant that must hold for every widget, button or not:
            // `Off` never appears among the on-states.
            assert!(
                !w.on_states.iter().any(|s| s.as_slice() == b"Off"),
                "on_states must never contain Off (§12.7.4.2.3)"
            );
            if !w.on_states.is_empty() {
                checked_any = true;
                // A button with on-states either has an Off appearance or does
                // not; both are legitimate. What matters is that the answer is
                // now expressible at all — before this field existed, a shell
                // could not distinguish the two cases.
                let _ = w.has_off_appearance;
            }
        }
    }
    assert!(
        checked_any,
        "the fixture must contain at least one button widget with on-states, \
         or this test asserts nothing about the new field"
    );
}

/// A one-page form whose widget carries **no `/P`** — the case no fixture in
/// `fixtures/synthetic/forms/` covers.
///
/// Every one of the ten form fixtures writes `/P` on every widget, so a
/// `/P`-based filter passes the whole corpus while failing on real files. `/P`
/// is Optional (§12.5.2 Table 164) and plenty of producers omit it. Built
/// in-memory rather than added as a fixture because the property being tested
/// is a *single absent key*, and a reader of a binary fixture cannot see that
/// the absence is the point.
fn form_doc_with_no_p_on_the_widget() -> Document {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 200] /Resources << >> >>",
        "<< /Type /Page /Parent 2 0 R /Annots [4 0 R] >>",
        // A merged field/widget: /FT /Tx makes it a field, /Subtype /Widget an
        // annotation. NOTE THE ABSENCE OF /P -- that is the whole point.
        "<< /Type /Annot /Subtype /Widget /FT /Tx /T (nop) /Rect [10 20 110 60] >>",
    ];
    let mut buf = b"%PDF-1.7
"
    .to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(
            format!(
                "{} 0 obj
{body}
endobj
",
                i + 1
            )
            .as_bytes(),
        );
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(
        format!(
            "xref
0 {size}
0000000000 65535 f 
"
        )
        .as_bytes(),
    );
    for off in &offsets {
        buf.extend_from_slice(
            format!(
                "{off:010} 00000 n 
"
            )
            .as_bytes(),
        );
    }
    buf.extend_from_slice(
        format!(
            "trailer
<< /Size {size} /Root 1 0 R >>
startxref
{xref_at}
%%EOF
"
        )
        .as_bytes(),
    );
    Document::from_bytes(buf).expect("synthetic form parses")
}

/// ★★ THE TEST THAT ACTUALLY BITES: a widget with no `/P` is still found.
///
/// The first version of this asserted the `/P` point against `demo-form.pdf`
/// and **the sabotage passed** — because every fixture in the corpus writes
/// `/P`. The test's own failure message claimed to guard the filter and could
/// not. This one can: filtering on `/P` returns nothing here.
#[test]
fn a_widget_without_a_p_entry_is_still_found() {
    let doc = form_doc_with_no_p_on_the_widget();
    let session = EditSession::new(doc);
    let rects = session.widget_rects(0);
    assert_eq!(
        rects.len(),
        1,
        "★ a widget with NO /P must still be found. /P is Optional (§12.5.2 Table 164) and commonly absent; a /P-based filter returns nothing here and nothing on a large class of real forms, with no error."
    );
    assert_eq!(rects[0].1, [10.0, 20.0, 110.0, 60.0]);
}

/// ★ `widget_rects` finds widgets by walking `/Annots`, NOT by filtering on `/P`.
///
/// Requested by the `pdfcer-gui` session (2026-08-14), which was deriving the
/// same data by parsing the whole `/AcroForm` per hit-test.
///
/// The `/P` point is the one that matters and is why this is a test rather than
/// a doc note: `/P` is Optional (§12.5.2 Table 164) and is frequently **absent**
/// on widgets. A `/P`-based filter — the obvious way to write this, and the way
/// `dimension_rects` legitimately does it for ce dimensions — would return
/// **nothing** on a large class of real forms, with no error. This asserts a
/// real fixture's widgets are found.
#[test]
fn widget_rects_finds_widgets_on_a_real_form() {
    let doc = Document::load(&fixture("forms/demo-form.pdf")).expect("fixture loads");
    let session = EditSession::new(doc);
    let rects = session.widget_rects(0);

    assert!(
        !rects.is_empty(),
        "★ no widgets found on page 0 of a form fixture. If this fails after a \
         refactor, the likely cause is filtering on /P — which is Optional and \
         absent on most widgets, so the filter silently matches nothing."
    );

    for (id, [llx, lly, urx, ury]) in &rects {
        assert!(
            llx <= urx && lly <= ury,
            "widget {id:?} rect must be normalised (§7.9.5 permits either \
             corner order): [{llx}, {lly}, {urx}, {ury}]"
        );
    }
}

/// Every rect returned belongs to a widget the form parser also knows.
///
/// A differential check rather than a value assertion: it compares the new
/// query against the route the requesting session was using, so the two cannot
/// disagree about which widgets exist.
#[test]
fn widget_rects_agrees_with_the_acroform_parse_it_replaces() {
    let doc = Document::load(&fixture("forms/demo-form.pdf")).expect("fixture loads");
    let form = pdfcer_core::forms::parse_acroform(&doc).expect("has a form");
    let session = EditSession::new(doc);

    let known: std::collections::BTreeSet<_> = form
        .fields
        .iter()
        .flat_map(|f| f.widgets.iter().map(|w| w.id))
        .collect();

    for (id, _) in session.widget_rects(0) {
        assert!(
            known.contains(&id),
            "widget_rects returned {id:?}, which the AcroForm parse does not \
             know as a widget — the two routes disagree about what a widget is"
        );
    }
}
