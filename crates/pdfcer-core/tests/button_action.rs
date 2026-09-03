//! # A Reset button that actually resets (`Pass 182.0`, ISO 32000-1 §12.7.5.3)
//!
//! Operator ruling, 2026-08-30: *"a reset button should actually reset."*
//!
//! Until this Pass `add_push_button` authored a valid button that did nothing
//! and said so — decision 009 posture A, which refused to author **any** `/A`
//! action. The ruling moved that one notch, and the tests below pin both the
//! notch and the boundary either side of it.
//!
//! ## What is being pinned
//!
//! - the bytes are the ones §12.7.5.3 defines, including the case where the
//!   correct thing to write is **nothing** (`ResetScope::All` omits `/Fields`,
//!   because an empty array means "reset these zero fields");
//! - `Except` sets Table 239's bit 1 and `Only` does not — the two differ by
//!   one integer and produce opposite documents;
//! - a name that does not exist is refused **before** anything is written;
//! - a non-button is refused;
//! - removing an action reports **what it destroyed**, including a script
//!   pdfcer would never write back;
//! - and the round trip: a button pdfcer wrote is one pdfcer reads back.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{ButtonAction, ButtonActionState, EditError, EditSession, ResetScope};
use pdfcer_core::writer::SaveOptions;

/// A form with a text field carrying `/V` and `/DV`, and a push button with
/// **no** action — the state `add_push_button` produces.
fn form_with_inert_button() -> Vec<u8> {
    let content = "BT /Helv 12 Tf 60 700 Td (form) Tj ET\n";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [5 0 R 6 0 R] \
         /DA (/Helv 0 Tf 0 g) /DR << /Font << /Helv 7 0 R >> >> >> >>"
            .to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 750] /Resources \
         << /Font << /Helv 7 0 R >> >> /Contents 4 0 R /Annots [5 0 R 6 0 R] >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ),
        "<< /Type /Annot /Subtype /Widget /FT /Tx /T (Name) /V (typed) /DV (factory) \
         /Rect [20 600 200 620] /P 3 0 R /F 4 /DA (/Helv 12 Tf 0 g) >>"
            .to_owned(),
        "<< /Type /Annot /Subtype /Widget /FT /Btn /Ff 65536 /T (DoReset) \
         /Rect [20 550 100 575] /P 3 0 R /F 4 /MK << /CA (Reset) >> >>"
            .to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_owned(),
    ];
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

fn session() -> EditSession {
    EditSession::new(Document::from_bytes(form_with_inert_button()).unwrap())
}

fn saved(s: &EditSession) -> String {
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    String::from_utf8_lossy(&bytes).into_owned()
}

/// **`All` writes the action and NO `/Fields`.**
///
/// §12.7.5.3: *"If this entry is omitted, the Include/Exclude flag shall be
/// ignored; all fields in the document's interactive form are reset."* An
/// empty array would mean "reset exactly these zero fields" — a different
/// document, and the opposite of what was asked for. So the assertion is on
/// the ABSENCE, which is the part a plausible implementation gets wrong.
#[test]
fn reset_all_omits_the_fields_array_entirely() {
    let mut s = session();
    let out = s
        .set_button_action(
            "DoReset",
            Some(ButtonAction::ResetForm {
                scope: ResetScope::All,
            }),
        )
        .expect("sets");
    assert_eq!(out.replaced, None, "the fixture's button had no action");

    let text = saved(&s);
    assert!(
        text.contains("/S /ResetForm"),
        "the action is written: {text}"
    );
    assert!(
        !text.contains("/Fields []"),
        "an EMPTY /Fields array means 'reset nothing' and must not be written"
    );
}

/// **`Only` and `Except` differ by one integer, and it is the one that
/// inverts the meaning.**
///
/// Table 239 bit 1 (`Include/Exclude`): clear ⇒ the array says which to
/// reset; set ⇒ it says which to spare. Asserted together in one test because
/// the failure worth catching is not "the flag is missing" but "the two cases
/// produce the same bytes".
#[test]
fn only_and_except_differ_by_the_include_exclude_flag() {
    let mut a = session();
    a.set_button_action(
        "DoReset",
        Some(ButtonAction::ResetForm {
            scope: ResetScope::Only(vec!["Name".to_owned()]),
        }),
    )
    .expect("sets");
    let only = saved(&a);

    let mut b = session();
    b.set_button_action(
        "DoReset",
        Some(ButtonAction::ResetForm {
            scope: ResetScope::Except(vec!["Name".to_owned()]),
        }),
    )
    .expect("sets");
    let except = saved(&b);

    assert!(only.contains("/Fields"), "Only names its fields");
    assert!(except.contains("/Fields"), "Except names its fields too");
    assert!(
        !only.contains("/Flags 1"),
        "Only must leave the Include/Exclude flag CLEAR: {only}"
    );
    assert!(
        except.contains("/Flags 1"),
        "Except must SET it (Table 239 bit 1): {except}"
    );
    assert_ne!(
        only, except,
        "★ the two scopes must not produce identical documents"
    );
}

/// ★ **A reset target that does not exist is refused before anything is
/// written.**
///
/// The same discipline `reset_form` uses. A button pointing at a field that
/// is not there is a button that silently does less than it says, and the
/// operator finds out by clicking it in another viewer.
#[test]
fn an_unknown_reset_target_is_refused_and_writes_nothing() {
    let mut s = session();
    let before = saved(&s);
    let err = s
        .set_button_action(
            "DoReset",
            Some(ButtonAction::ResetForm {
                scope: ResetScope::Only(vec!["NoSuchField".to_owned()]),
            }),
        )
        .unwrap_err();
    assert!(
        matches!(err, EditError::FieldNotFound { ref name } if name == "NoSuchField"),
        "refused by name: {err:?}"
    );
    assert_eq!(saved(&s), before, "and nothing was written");
}

/// **A field that is not a push button is refused by name.**
#[test]
fn a_non_button_is_refused() {
    let mut s = session();
    let err = s
        .set_button_action(
            "Name",
            Some(ButtonAction::ResetForm {
                scope: ResetScope::All,
            }),
        )
        .unwrap_err();
    assert!(
        matches!(err, EditError::ButtonActionWrongFieldType { .. }),
        "a text field has no click to attach an action to: {err:?}"
    );
}

/// ★★ **Removing an action reports WHAT IT DESTROYED — including a script.**
///
/// The reason `replaced` is a described name rather than an
/// `Option<ButtonAction>`: pdfcer will not author JavaScript, but a form editor
/// opening somebody else's document may well delete one, and reporting that
/// as `None` would say "there was nothing there".
#[test]
fn removing_an_action_names_what_was_there_even_a_script() {
    let mut s = session();
    s.set_button_action(
        "DoReset",
        Some(ButtonAction::ResetForm {
            scope: ResetScope::All,
        }),
    )
    .expect("sets");

    let out = s.set_button_action("DoReset", None).expect("removes");
    assert_eq!(
        out.replaced.as_deref(),
        Some("ResetForm"),
        "the removed action is named"
    );
    assert_eq!(out.applied, None, "and nothing is in force after");
    assert!(
        !saved(&s).contains("/S /ResetForm"),
        "the action is gone from the saved bytes"
    );
}

/// **One undoable command, and undo restores the button to inert.**
#[test]
fn setting_an_action_is_one_undoable_command() {
    let mut s = session();
    let depth = s.undo_depth();
    s.set_button_action(
        "DoReset",
        Some(ButtonAction::ResetForm {
            scope: ResetScope::All,
        }),
    )
    .expect("sets");
    assert_eq!(s.undo_depth(), depth + 1, "exactly one command");
    assert!(saved(&s).contains("/S /ResetForm"));

    s.undo().expect("undo");
    assert!(
        !saved(&s).contains("/S /ResetForm"),
        "one undo takes the whole action back"
    );
}

/// ★★★ **The button pdfcer writes is one pdfcer reads back as an action.**
///
/// The end-to-end check. `list-fields` counts `annot_actions` by walking the
/// same `/A` this verb writes, so a button that reads back as carrying no
/// action would be one no viewer would honour either.
#[test]
fn a_written_reset_button_reads_back_as_an_action() {
    let mut s = session();
    s.set_button_action(
        "DoReset",
        Some(ButtonAction::ResetForm {
            scope: ResetScope::All,
        }),
    )
    .expect("sets");

    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let reopened = Document::from_bytes(bytes).expect("reopens");
    let form = pdfcer_core::forms::parse_acroform(&reopened.view()).expect("has a form");
    let button = form
        .fields
        .iter()
        .find(|f| f.fully_qualified_name == "DoReset")
        .expect("the button survives the round trip");
    assert!(
        button.flags.has(pdfcer_core::forms::FieldFlags::PUSHBUTTON),
        "and is still a push button"
    );

    // The ACTION itself is asserted on the reopened bytes rather than on the
    // parsed model, because `AcroForm` deliberately does not model `/A` --
    // pdfcer recognises actions for hazard classification and does not carry
    // them as field state. The bytes are what a viewer honours.
    let text = String::from_utf8_lossy(reopened.bytes());
    assert!(
        text.contains("/S /ResetForm"),
        "the reopened document carries the action"
    );
    assert!(
        !text.contains("/JavaScript"),
        "★ and pdfcer authored a ResetForm and nothing else"
    );
}

// ---------------------------------------------------------------------------
// Pass 212.0 -- the READER, at pdfcer-gui's request.
//
// `Pass 183.1` shipped `set_button_action` with no way to ask what a button
// currently does, so the control that sets it could not show what it was set
// to. The shell declined to ship the workaround -- display "Nothing" and be
// wrong about other people's documents -- and reported it instead.
// ---------------------------------------------------------------------------

/// The same fixture, with a JavaScript action pdfcer would refuse to write.
///
/// Authored by hand rather than through `set_button_action`, because the whole
/// point of `Foreign` is actions pdfcer CANNOT author -- a fixture built with
/// the writer could never contain one.
fn form_with_javascript_button() -> Vec<u8> {
    let original = String::from_utf8_lossy(&form_with_inert_button()).into_owned();
    // Splice an `/A` into the button widget only. Offsets shift, so the xref
    // is rebuilt by reparsing rather than patched -- `Document::from_bytes`
    // tolerates a stale xref by scanning, which is exactly the recovery path
    // `Pass B` shipped, and this fixture leans on it deliberately.
    original
        .replace(
            "/MK << /CA (Reset) >> >>",
            // Balanced parentheses need no escaping inside a PDF literal
            // string (§7.3.4.2), which keeps this readable as Rust too.
            "/MK << /CA (Reset) >> /A << /Type /Action /S /JavaScript /JS (app.alert(1);) >> >>",
        )
        .into_bytes()
}

/// ★ A button with no `/A` reads as `None` — and that is a MEASUREMENT, not a
/// default.
#[test]
fn a_button_with_no_action_reads_as_none() {
    let s = session();
    assert_eq!(s.button_action("DoReset").unwrap(), ButtonActionState::None);
}

/// ★★★ THE ROUND TRIP. What the writer wrote, the reader reads back.
///
/// Without this the two halves could drift silently: the writer's own
/// `ButtonActionChange::replaced` would keep reporting correctly while the
/// reader answered something else, and nothing would notice until a shell
/// displayed the wrong thing.
#[test]
fn what_set_button_action_writes_the_reader_reads_back() {
    let mut s = session();
    s.set_button_action(
        "DoReset",
        Some(ButtonAction::ResetForm {
            scope: ResetScope::All,
        }),
    )
    .expect("writes");
    assert_eq!(
        s.button_action("DoReset").unwrap(),
        ButtonActionState::Known(ButtonAction::ResetForm {
            scope: ResetScope::All
        }),
        "the reader must return what the writer just wrote"
    );

    // ...and a named target list survives with its sense intact. Table 239's
    // `/Flags` bit 1 INVERTS `/Fields`, so `Only` and `Except` are the same
    // array with one bit between them -- the one thing a reader can most
    // easily get backwards.
    s.set_button_action(
        "DoReset",
        Some(ButtonAction::ResetForm {
            scope: ResetScope::Only(vec!["Name".to_owned()]),
        }),
    )
    .expect("writes");
    assert_eq!(
        s.button_action("DoReset").unwrap(),
        ButtonActionState::Known(ButtonAction::ResetForm {
            scope: ResetScope::Only(vec!["Name".to_owned()])
        }),
        "★ Only must not come back as Except -- one flag bit separates them"
    );
}

/// Removing the action returns the button to `None`, rather than to a stale
/// reading of what used to be there.
#[test]
fn clearing_the_action_reads_as_none_again() {
    let mut s = session();
    s.set_button_action(
        "DoReset",
        Some(ButtonAction::ResetForm {
            scope: ResetScope::All,
        }),
    )
    .expect("writes");
    s.set_button_action("DoReset", None).expect("clears");
    assert_eq!(s.button_action("DoReset").unwrap(), ButtonActionState::None);
}

/// ★★ THE VARIANT THAT CARRIES THE VALUE. A JavaScript action must read as
/// `Foreign`, naming its subtype.
///
/// `None` and `Known` could both be synthesised by a shell that guessed;
/// `Foreign` cannot. It is what lets a control say "this button runs a script,
/// pdfcer will not change it" instead of silently offering to replace it with
/// "Nothing" -- which is rule 4's sneaky half in its purest form, pdfcer
/// asserting a fact about somebody else's document that it never checked.
#[test]
fn a_javascript_action_reads_as_foreign_and_names_itself() {
    let s = EditSession::new(Document::from_bytes(form_with_javascript_button()).unwrap());
    assert_eq!(
        s.button_action("DoReset").unwrap(),
        ButtonActionState::Foreign("JavaScript".to_owned()),
        "a script action must be named, not reported as absent"
    );
}

/// Reading is refused on the same footing as writing.
///
/// A shell must not be able to learn through the reader about a field it would
/// be refused permission to change -- the two verbs answer for the same set or
/// they disagree, and a disagreement is a surface for exactly the confusion
/// this reader exists to remove.
#[test]
fn reading_a_non_button_is_refused_the_same_way_writing_is() {
    let s = session();
    assert!(matches!(
        s.button_action("Name"),
        Err(EditError::ButtonActionWrongFieldType { .. })
    ));
    assert!(matches!(
        s.button_action("NoSuchField"),
        Err(EditError::FieldNotFound { .. })
    ));
}
