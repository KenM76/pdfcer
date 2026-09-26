//! Tests for `pdfcer_text::text_state`, run against its public API.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]

use pdfcer_text::text_state::*;

#[test]
fn table_105_initial_values() {
    let ts = AmbientTextState::initial();
    let p = ts.params();
    assert_eq!(p.char_spacing, 0.0);
    assert_eq!(p.word_spacing, 0.0);
    assert_eq!(p.h_scale, 1.0, "Tz initial 100 ⇒ Th 1.0");
    assert_eq!(p.leading, 0.0);
    assert_eq!(p.rise, 0.0);
    assert_eq!(p.render_mode, 0);
}

#[test]
fn unset_parameters_restore_to_the_spec_default() {
    let ts = AmbientTextState::initial();
    assert_eq!(
        ts.restore_bytes(TextStateParam::CharSpacing).unwrap(),
        b"0 Tc"
    );
    assert_eq!(
        ts.restore_bytes(TextStateParam::WordSpacing).unwrap(),
        b"0 Tw"
    );
    assert_eq!(
        ts.restore_bytes(TextStateParam::HorizScale).unwrap(),
        b"100 Tz"
    );
    assert_eq!(ts.restore_bytes(TextStateParam::Leading).unwrap(), b"0 TL");
    assert_eq!(ts.restore_bytes(TextStateParam::Rise).unwrap(), b"0 Ts");
    assert_eq!(
        ts.restore_bytes(TextStateParam::RenderMode).unwrap(),
        b"0 Tr"
    );
}

/// The tier-2 promise: an observed value restores as the bytes that
/// were written, NOT a renormalized rendering of the parsed number.
#[test]
fn observed_parameters_restore_the_raw_operand_bytes() {
    let mut ts = AmbientTextState::initial();
    assert!(ts.apply_operator(b"Tc", &[0.5], b"0.5000 Tc"));
    assert!(ts.apply_operator(b"Ts", &[3.0], b"+3.0 Ts"));
    assert_eq!(
        ts.restore_bytes(TextStateParam::CharSpacing).unwrap(),
        b"0.5000 Tc",
        "a trailing-zero spelling must survive a restore verbatim"
    );
    assert_eq!(ts.restore_bytes(TextStateParam::Rise).unwrap(), b"+3.0 Ts");
    assert_eq!(ts.params().char_spacing, 0.5);
    assert_eq!(ts.params().rise, 3.0);
}

/// Table 109's `"` sets `Tw` and `Tc` **and shows a string**, so its
/// bytes are not a restore. This is the trap the `ObservedIndirect`
/// tier exists to stop: a naive tier-2 restore would re-paint the text.
#[test]
fn double_quote_sets_both_spacings_but_is_not_byte_restorable() {
    let mut ts = AmbientTextState::initial();
    assert!(ts.apply_operator(b"\"", &[2.0, 0.25], b"2 0.25 (hi) \""));
    assert_eq!(ts.params().word_spacing, 2.0);
    assert_eq!(ts.params().char_spacing, 0.25);

    assert!(!ts.word_spacing.is_byte_faithful());
    assert!(ts.word_spacing.is_restorable());
    assert_eq!(
        ts.restore_bytes(TextStateParam::WordSpacing).unwrap(),
        b"2 Tw",
        "the restore must be a re-spelling, NEVER the `\"` bytes — those \
         would show the string a second time"
    );
    assert_eq!(
        ts.restore_bytes(TextStateParam::CharSpacing).unwrap(),
        b"0.25 Tc"
    );
}

/// `TD` sets `TL` **and moves to the next line** (Table 108) — the
/// other member of the `ObservedIndirect` class. Re-emitting the `TD`
/// to restore leading would displace every following glyph.
#[test]
fn td_derived_leading_restores_as_tl_not_as_td() {
    let mut ts = AmbientTextState::initial();
    ts.set_indirect(TextStateParam::Leading, 14.0, "TD");
    assert_eq!(ts.params().leading, 14.0);
    assert!(!ts.leading.is_byte_faithful());
    assert_eq!(ts.restore_bytes(TextStateParam::Leading).unwrap(), b"14 TL");
}

/// An indirectly-set value is still *set*, so descending into a form
/// makes it unrestorable for the same reason a directly-set one is.
#[test]
fn an_indirect_value_also_becomes_unobservable_inside_a_form() {
    let mut ts = AmbientTextState::initial();
    ts.set_indirect(TextStateParam::Leading, 14.0, "TD");
    ts.enter_form(None);
    assert!(!ts.leading.is_restorable());
    let err = ts.restore_bytes(TextStateParam::Leading).unwrap_err();
    assert!(err.to_string().contains("leading"), "{err}");
}

#[test]
fn non_text_state_operators_are_not_claimed() {
    let mut ts = AmbientTextState::initial();
    assert!(!ts.apply_operator(b"Tf", &[12.0], b"/F1 12 Tf"));
    assert!(!ts.apply_operator(b"Tj", &[], b"(hi) Tj"));
    assert!(!ts.apply_operator(b"q", &[], b"q"));
    assert_eq!(ts, AmbientTextState::initial());
}

/// Tier 3. The value stays known (the form inherits it, §8.10.1); only
/// the ability to RESTORE it is lost.
#[test]
fn form_xobject_inheritance_refuses_rather_than_guessing() {
    let mut ts = AmbientTextState::initial();
    ts.apply_operator(b"Tc", &[0.5], b"0.5 Tc");
    ts.enter_form(Some(12));

    let err = ts.restore_bytes(TextStateParam::CharSpacing).unwrap_err();
    assert!(matches!(
        err,
        AmbientRestoreError::Unobservable {
            param: TextStateParam::CharSpacing,
            reason: UnobservableAmbient::FormXObject { object: Some(12) },
        }
    ));
    let msg = err.to_string();
    assert!(msg.contains("character spacing"), "{msg}");
    assert!(msg.contains("form XObject 12"), "{msg}");
    assert_eq!(ts.params().char_spacing, 0.5, "the value is still known");
}

/// A parameter no operator ever set is at its Table 105 default
/// everywhere, so descending into a form does NOT make it unobservable.
#[test]
fn form_xobject_leaves_never_set_parameters_restorable() {
    let mut ts = AmbientTextState::initial();
    ts.enter_form(Some(3));
    assert_eq!(ts.restore_bytes(TextStateParam::Rise).unwrap(), b"0 Ts");
    assert!(ts.rise.is_restorable());
}

/// A value set INSIDE the form is observable in the form's own buffer,
/// so it overwrites the inherited mark.
#[test]
fn a_value_set_inside_the_form_becomes_restorable_again() {
    let mut ts = AmbientTextState::initial();
    ts.apply_operator(b"Ts", &[2.0], b"2 Ts");
    ts.enter_form(Some(3));
    assert!(!ts.rise.is_restorable());
    ts.apply_operator(b"Ts", &[4.0], b"4 Ts");
    assert!(ts.rise.is_restorable());
    assert_eq!(ts.restore_bytes(TextStateParam::Rise).unwrap(), b"4 Ts");
}

#[test]
fn operator_names_round_trip_through_the_parameter_enum() {
    for param in TextStateParam::ALL {
        assert_eq!(TextStateParam::from_operator(param.operator()), Some(param));
        // The spec-default byte string must actually be that operator.
        let bytes = param.initial_restore_bytes();
        assert!(
            bytes.ends_with(param.operator()),
            "{param} default bytes {:?}",
            String::from_utf8_lossy(bytes)
        );
    }
}
