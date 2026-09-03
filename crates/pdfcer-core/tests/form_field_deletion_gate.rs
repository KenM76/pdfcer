//! The certification gate that separates FILLING from DELETING (§12.8.4).
//!
//! ## Why this file exists at all
//!
//! [`EditSession::fill_refusal`] and [`EditSession::deletion_refusal`] look
//! like the same query and are not. Filling takes the **`/P`-aware** gate,
//! because a certified document at `/P >= 2` is frequently certified
//! precisely TO allow form filling (§12.8.4 Table 254). Deleting a field is a
//! **structural** change to the form, which is what a certification signature
//! exists to freeze, so it takes the **strict** gate.
//!
//! The consequence a shell must not get wrong: **there are documents where
//! pdfcer offers filling and refuses deletion**, and they are the ordinary
//! case — a certified fillable form. A GUI that reused `fill_refusal` to
//! decide whether to enable a delete control would offer a button whose every
//! press returns the same error.
//!
//! ## Why the corpus needed a new fixture for this (R162)
//!
//! Every certification fixture before `certified-p2-form.pdf` was `/P 1` —
//! "no changes permitted" — which refuses **both** operations. A test written
//! against `/P 1` passes whether or not the two gates differ at all, and
//! would go on passing if someone collapsed `deletion_refusal` into
//! `fill_refusal` tomorrow. It is an assertion that cannot come out false.
//!
//! `/P 2` is the only value where the two disagree, so it is the only value
//! that tests the distinction.
//!
//! ## The three cases, and why all three are needed
//!
//! Each one exists to stop a different way of passing vacuously:
//!
//! 1. **`/P 2`** — the divergence itself. Fill permitted, deletion refused.
//! 2. **`/P 1`** — both refused. Without it, case 1 is consistent with
//!    `deletion_refusal` simply being stricter *by accident* rather than by
//!    the certification level.
//! 3. **an uncertified form** — both permitted. Without it, cases 1 and 2
//!    are both consistent with `deletion_refusal` returning `Some` for every
//!    document ever handed to it, which would make the gate useless and the
//!    tests green.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(name)
}

fn session(rel: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(rel)).expect("load fixture"))
}

/// `/P 2`: **filling is offered, deletion is refused.** The whole point.
#[test]
fn a_p2_certified_form_permits_filling_and_refuses_deletion() {
    let s = session("forms/certified-p2-form.pdf");

    assert!(
        s.fill_refusal().is_none(),
        "/P 2 means 'filling in forms and signing is permitted' (§12.8.4 \
         Table 254), so the FILL gate must let this through — if it does \
         not, this fixture no longer separates the two gates and every \
         assertion below it is testing something else",
    );

    let refusal = s.deletion_refusal().expect(
        "/P 2 permits FILLING, not structural change. Deleting a field \
         restructures the form, which is exactly what a certification \
         signature freezes, so the STRICT gate must refuse it here. If this \
         is None, deletion_refusal has been collapsed into fill_refusal and \
         a certified form can now have its fields removed.",
    );
    assert!(
        matches!(refusal, EditError::CertificationForbidsChange { .. }),
        "the refusal must name the certification as the cause so a shell can \
         say WHY, not merely that something failed; got {refusal:?}",
    );

    // And the verbs themselves must agree with the query. A gate that
    // reports a refusal the operation does not actually make is worse than
    // no gate: it disables a control that would have worked.
    let mut s = s;
    let err = s
        .delete_field("FullName")
        .expect_err("delete_field must refuse what deletion_refusal predicted");
    assert!(
        matches!(err, EditError::CertificationForbidsChange { .. }),
        "the query and the verb must refuse for the SAME reason, or the \
         disabled control explains itself with the wrong sentence; got \
         {err:?}",
    );
}

/// `/P 1`: both refused — so case 1's split is caused by the LEVEL.
#[test]
fn a_p1_certified_document_refuses_both() {
    let s = session("addtext/certified-locked.pdf");
    assert!(
        s.fill_refusal().is_some(),
        "/P 1 is 'no changes permitted', so even filling must refuse",
    );
    assert!(
        s.deletion_refusal().is_some(),
        "/P 1 must refuse deletion too — a gate that refuses at /P 2 but \
         permits at /P 1 would be inverted",
    );
}

/// An uncertified form: **both permitted.**
///
/// R162's control. Without this, both tests above are equally consistent
/// with `deletion_refusal` returning `Some` unconditionally — which would
/// disable the delete control on every document pdfcer ever opens, while
/// every assertion in this file stayed green.
#[test]
fn an_uncertified_form_permits_both_so_the_gate_is_not_always_on() {
    let s = session("forms/demo-form.pdf");
    assert!(
        s.fill_refusal().is_none(),
        "an ordinary form has nothing to refuse filling",
    );
    assert!(
        s.deletion_refusal().is_none(),
        "an ordinary form has nothing to refuse deletion — if this refuses, \
         the gate is stuck on and the two tests above prove nothing",
    );
}
