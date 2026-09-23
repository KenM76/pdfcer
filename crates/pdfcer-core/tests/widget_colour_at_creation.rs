//! **Colour BEFORE placement: the five `New*` creation specs carry `/MK`
//! `/BG` and `/BC`** (`Pass 308.1`, request `G020`).
//!
//! ## What was missing
//!
//! `Pass 308.0` made the appearance builders paint `/MK`, and `Pass 308.2`
//! gave the edit verb a third outcome state. Both act **after** placement.
//! The operator's words were *"before or after placement"*, and before was
//! create-then-edit — two commands, two undo entries, and an appearance
//! written once in the wrong colour and once in the right one.
//!
//! None of the five specs had a colour field. Now each does, and the value
//! reaches the `/MK` dictionary and the `/AP` artwork from **one place**, so
//! they cannot be written out of step.
//!
//! ## The phantom border, which is the interesting half
//!
//! `add_text_field` and `add_choice_field` wrote `/MK` `/BC [0 0 0]` and
//! handed the appearance builder nothing. The dictionary claimed a black
//! frame; the `/AP` drew none. That cost nothing for as long as nothing read
//! one to produce the other — and `Pass 308.0` made `edit_widget` read
//! exactly that key, so the first **resize** of a created text field would
//! have materialised a frame the operator never asked for.
//!
//! The fix is to stop claiming it, not to start drawing it: *"a text field
//! draws no box by default"* is a stated invariant of
//! [`pdfcer_core::annot_author::build_field_text_appearance`], and
//! `widget_colour_appearance.rs` asserts it. So the creation floor states
//! **no colour at all** for four of the five types, the push button keeps its
//! plate, and every appearance pdfcer authors is byte-identical to before.
//!
//! That is the same shape `Pass 308.0` recorded from the other side: **two
//! representations of one fact can disagree indefinitely at no cost, and the
//! cost arrives in full the moment a third thing derives one from the other.**
//!
//! ## What is asserted
//!
//! The **operators in the appearance stream**, not the model — a model
//! assertion passes on a build that writes `/MK` and paints nothing, which is
//! precisely the defect (`R159`). The dictionary is asserted beside it, so a
//! build that paints without recording fails too.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{
    AppearanceOutcome, ChoiceOption, EditSession, MkColorEdit, NewCheckBox, NewChoiceField,
    NewPushButton, NewRadioButton, NewTextField, WidgetEdit,
};
use pdfcer_core::forms::{self, MkColor};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::Object;
use pdfcer_core::page_tree::Rect;
use std::path::{Path, PathBuf};

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn session() -> EditSession {
    EditSession::new(Document::load(&fixture("dimension/plain-base.pdf")).unwrap())
}

fn rect() -> Rect {
    Rect {
        llx: 20.0,
        lly: 100.0,
        urx: 60.0,
        ury: 124.0,
    }
}

fn field_named(s: &EditSession, name: &str) -> forms::Field {
    forms::parse_acroform(&s.graph())
        .expect("an AcroForm")
        .fields
        .into_iter()
        .find(|f| f.fully_qualified_name == name)
        .expect("the field")
}

/// Every appearance stream reachable from widget `index`'s `/AP` `/N`, joined.
///
/// Handles both `/N` shapes — a stream (text, choice, push button) and a
/// state sub-dictionary (check box, radio) — for the reason
/// `widget_colour_appearance.rs` gives: a reader that understood only one
/// would silently assert nothing about the other.
fn ap_text_of(s: &EditSession, name: &str, index: usize) -> String {
    let g = s.graph();
    let field = field_named(s, name);
    let dict = g
        .resolved(field.widgets[index].id)
        .as_dict()
        .cloned()
        .expect("widget dict");
    let Some(Object::Dict(ap)) = dict.get(b"AP").map(|o| g.resolve(o).clone()) else {
        return String::new();
    };
    let Some(n) = ap.get(b"N") else {
        return String::new();
    };
    let mut streams = Vec::new();
    match g.resolve(n).clone() {
        Object::Stream(st) => streams.push(st),
        Object::Dict(states) => {
            for (_, v) in &states.0 {
                if let Object::Stream(st) = g.resolve(v).clone() {
                    streams.push(st);
                }
            }
        }
        _ => {}
    }
    streams
        .into_iter()
        .map(|st| {
            String::from_utf8_lossy(s.view().slice(st.data_span).unwrap_or_default()).into_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn ap_text(s: &EditSession, name: &str) -> String {
    ap_text_of(s, name, 0)
}

/// Whether the widget carries an `/MK` dictionary at all.
fn has_mk(s: &EditSession, name: &str) -> bool {
    let g = s.graph();
    let field = field_named(s, name);
    g.resolved(field.widgets[0].id)
        .as_dict()
        .is_some_and(|d| d.get(b"MK").is_some())
}

// -------------------------------------------------------------------------
// The ask: a colour chosen BEFORE placement lands in one command
// -------------------------------------------------------------------------

#[test]
fn a_text_field_is_created_in_the_colours_it_was_asked_for() {
    let mut s = session();
    s.add_text_field(
        &NewTextField::new(0, "Name", rect())
            .declining_tooltip()
            .with_background(MkColor::Rgb(0.9, 0.95, 1.0))
            .with_border_color(MkColor::Rgb(0.0, 0.0, 0.5)),
    )
    .unwrap();

    // The artwork — the half a broken build gets wrong.
    let art = ap_text(&s, "Name");
    assert!(art.contains("0.9 0.95 1 rg"), "the fill: {art}");
    assert!(art.contains("0 0 0.5 RG"), "the frame: {art}");

    // And the dictionary, which a build that paints without recording misses.
    let w = &field_named(&s, "Name").widgets[0];
    assert_eq!(w.background, Some(MkColor::Rgb(0.9, 0.95, 1.0)));
    assert_eq!(w.border_color, Some(MkColor::Rgb(0.0, 0.0, 0.5)));
}

#[test]
fn a_check_box_is_created_in_the_colours_it_was_asked_for() {
    let mut s = session();
    s.add_check_box(
        &NewCheckBox::new(0, "Agree", rect())
            .declining_tooltip()
            .with_background(MkColor::Gray(0.9))
            .with_border_color(MkColor::Rgb(0.8, 0.0, 0.0)),
    )
    .unwrap();

    let art = ap_text(&s, "Agree");
    assert!(art.contains("0.9 g"), "the box fill: {art}");
    assert!(art.contains("0.8 0 0 RG"), "the box frame: {art}");
    // The tick takes the BORDER colour — `/MK` has no third colour meaning
    // "the mark", and a tick in the background colour is invisible against
    // the box it sits in.
    assert!(art.contains("0.8 0 0 rg"), "and the tick's ink: {art}");

    let w = &field_named(&s, "Agree").widgets[0];
    assert_eq!(w.background, Some(MkColor::Gray(0.9)));
    assert_eq!(w.border_color, Some(MkColor::Rgb(0.8, 0.0, 0.0)));
}

#[test]
fn a_radio_button_is_created_in_the_colours_it_was_asked_for() {
    let mut s = session();
    s.add_radio_button(
        &NewRadioButton::new(0, "Choice", rect(), "A")
            .declining_tooltip()
            .with_background(MkColor::Gray(0.9))
            .with_border_color(MkColor::Rgb(0.0, 0.5, 0.0)),
    )
    .unwrap();

    let art = ap_text(&s, "Choice");
    assert!(art.contains("0.9 g"), "the disc: {art}");
    assert!(art.contains("0 0.5 0 RG"), "the ring: {art}");

    let w = &field_named(&s, "Choice").widgets[0];
    assert_eq!(w.background, Some(MkColor::Gray(0.9)));
    assert_eq!(w.border_color, Some(MkColor::Rgb(0.0, 0.5, 0.0)));
}

#[test]
fn a_choice_field_is_created_in_the_colours_it_was_asked_for() {
    let mut s = session();
    s.add_choice_field(
        &NewChoiceField::new(
            0,
            "Country",
            rect(),
            vec![ChoiceOption::plain("Canada"), ChoiceOption::plain("Mexico")],
        )
        .declining_tooltip()
        .with_background(MkColor::Cmyk(0.0, 0.1, 0.2, 0.0))
        .with_border_color(MkColor::Gray(0.25)),
    )
    .unwrap();

    let art = ap_text(&s, "Country");
    // CMYK is PAINTED as CMYK, never converted — pdfcer owns no rendering
    // intent for a widget's chrome.
    assert!(art.contains("0 0.1 0.2 0 k"), "the fill stays CMYK: {art}");
    assert!(!art.contains(" rg"), "nothing was converted: {art}");
    assert!(art.contains("0.25 G"), "the frame: {art}");
}

#[test]
fn a_push_button_is_created_in_the_colours_it_was_asked_for() {
    let mut s = session();
    s.add_push_button(
        &NewPushButton::new(0, "Go", rect(), "Submit")
            .declining_tooltip()
            .with_background(MkColor::Rgb(0.1, 0.3, 0.7))
            .with_border_color(MkColor::Gray(1.0)),
    )
    .unwrap();

    let art = ap_text(&s, "Go");
    assert!(art.contains("0.1 0.3 0.7 rg"), "the plate: {art}");
    assert!(art.contains("1 G"), "the keyline: {art}");
    assert!(
        !art.contains("0.85 g"),
        "and the default plate grey is GONE, not painted under it: {art}"
    );

    let w = &field_named(&s, "Go").widgets[0];
    assert_eq!(w.background, Some(MkColor::Rgb(0.1, 0.3, 0.7)));
    assert_eq!(w.border_color, Some(MkColor::Gray(1.0)));
}

#[test]
fn a_push_button_can_be_created_with_no_plate_at_all() {
    // Table 189's EMPTY ARRAY — *"no colour"* — is expressible at creation
    // for the same reason it is expressible at edit: an absent key falls back
    // to the plate grey, and only the empty one says there is no plate.
    let mut s = session();
    s.add_push_button(
        &NewPushButton::new(0, "Go", rect(), "Submit")
            .declining_tooltip()
            .with_background(MkColor::None),
    )
    .unwrap();

    let art = ap_text(&s, "Go");
    assert!(!art.contains("0.85 g"), "no plate: {art}");
    assert!(art.contains("0 G"), "the keyline survives: {art}");
    assert_eq!(
        field_named(&s, "Go").widgets[0].background,
        Some(MkColor::None)
    );
}

// -------------------------------------------------------------------------
// The phantom `/BC [0 0 0]` is retired
// -------------------------------------------------------------------------

#[test]
fn a_created_text_field_no_longer_claims_a_border_it_does_not_draw() {
    let mut s = session();
    s.add_text_field(&NewTextField::new(0, "Name", rect()).declining_tooltip())
        .unwrap();

    // The artwork is unchanged from before this Pass: one `re W n` clip from
    // the variable-text generator, and nothing painted.
    let art = ap_text(&s, "Name");
    assert!(art.contains("re\nW\nn"), "the text clip is expected: {art}");
    assert!(!art.contains("\nf\n"), "nothing is filled: {art}");
    assert!(!art.contains("\nS\n"), "and nothing is stroked: {art}");

    // What changed: the dictionary no longer says otherwise.
    let w = &field_named(&s, "Name").widgets[0];
    assert_eq!(
        w.border_color, None,
        "the phantom /MK /BC [0 0 0] is gone — it claimed a frame the /AP \
         never drew, and Pass 308.0 made edit_widget read it"
    );
    assert_eq!(w.background, None);
    assert!(
        !has_mk(&s, "Name"),
        "with neither colour stated there is no /MK dictionary to write"
    );
}

#[test]
fn resizing_a_created_text_field_does_not_conjure_a_frame() {
    // THE DEFECT THIS PASS CLOSES, end to end. With the phantom `/BC` in the
    // dictionary, `edit_widget` — which regenerates from `/MK` since
    // `Pass 308.0` — would redraw this field WITH a black frame that was
    // never in its appearance.
    let mut s = session();
    s.add_text_field(&NewTextField::new(0, "Name", rect()).declining_tooltip())
        .unwrap();
    s.edit_widget(
        "Name",
        0,
        &WidgetEdit::new().with_rect(Rect {
            llx: 20.0,
            lly: 100.0,
            urx: 140.0,
            ury: 124.0,
        }),
    )
    .unwrap();

    let art = ap_text(&s, "Name");
    assert!(
        !art.contains("\nS\n"),
        "a resize must not add a frame the field never had: {art}"
    );
}

#[test]
fn a_created_choice_field_drops_the_phantom_border_too() {
    let mut s = session();
    s.add_choice_field(
        &NewChoiceField::new(0, "Country", rect(), vec![ChoiceOption::plain("Canada")])
            .declining_tooltip(),
    )
    .unwrap();

    let art = ap_text(&s, "Country");
    assert!(!art.contains("\nS\n"), "nothing is stroked: {art}");
    assert_eq!(field_named(&s, "Country").widgets[0].border_color, None);
    assert!(!has_mk(&s, "Country"));
}

// -------------------------------------------------------------------------
// The half that must not change: stating nothing is byte-identical
// -------------------------------------------------------------------------

#[test]
fn a_check_box_created_with_no_opinion_is_unchanged() {
    // A check box has always stroked black and filled nothing while writing
    // NO `/BC`. Threading a colour through must not add either a key or a
    // paint operator when nobody asked for one.
    let mut s = session();
    s.add_check_box(&NewCheckBox::new(0, "Agree", rect()).declining_tooltip())
        .unwrap();

    let art = ap_text(&s, "Agree");
    assert!(
        art.contains("0 G"),
        "the black border it always drew: {art}"
    );
    assert!(!art.contains(" rg"), "and no fill: {art}");

    let w = &field_named(&s, "Agree").widgets[0];
    assert_eq!(w.background, None);
    assert_eq!(w.border_color, None, "no /BC key, exactly as before");
    // `/MK` survives, because the tick style still lives in `/CA`.
    assert!(has_mk(&s, "Agree"));
}

#[test]
fn a_radio_button_created_with_no_opinion_writes_no_mk_at_all() {
    let mut s = session();
    s.add_radio_button(&NewRadioButton::new(0, "Choice", rect(), "A").declining_tooltip())
        .unwrap();

    let art = ap_text(&s, "Choice");
    assert!(art.contains("0 G"), "the black ring it always drew: {art}");
    assert!(!art.contains(" rg"), "and no disc fill: {art}");
    assert!(
        !has_mk(&s, "Choice"),
        "a radio button wrote no /MK before this Pass and still writes none"
    );
}

#[test]
fn a_push_button_created_with_no_opinion_keeps_its_plate() {
    let mut s = session();
    s.add_push_button(&NewPushButton::new(0, "Go", rect(), "Submit").declining_tooltip())
        .unwrap();

    let art = ap_text(&s, "Go");
    assert!(art.contains("0.85 g"), "the plate grey: {art}");
    assert!(art.contains("0 G"), "and the black keyline: {art}");

    let w = &field_named(&s, "Go").widgets[0];
    assert_eq!(w.background, Some(MkColor::Gray(0.85)));
    assert_eq!(w.border_color, Some(MkColor::Gray(0.0)));
}

#[test]
fn stating_the_push_buttons_own_defaults_is_identical_to_stating_nothing() {
    // The test a new parameter owes: its ABSENCE must be byte-identical to
    // before, and its explicit presence at the floor value must be identical
    // to its absence. If those two diverge, one of the two paths is inventing
    // something.
    let mut plain = session();
    plain
        .add_push_button(&NewPushButton::new(0, "Go", rect(), "Submit").declining_tooltip())
        .unwrap();

    let mut explicit = session();
    explicit
        .add_push_button(
            &NewPushButton::new(0, "Go", rect(), "Submit")
                .declining_tooltip()
                .with_background(MkColor::Gray(0.85))
                .with_border_color(MkColor::Gray(0.0)),
        )
        .unwrap();

    assert_eq!(ap_text(&plain, "Go"), ap_text(&explicit, "Go"));
}

// -------------------------------------------------------------------------
// It survives everything that comes after
// -------------------------------------------------------------------------

#[test]
fn a_creation_colour_survives_a_resize_rather_than_being_redrawn_grey() {
    // The ownership test asks *"would pdfcer draw exactly these bytes?"* and
    // is handed the widget's colours AS STORED. A creation-time colour that
    // reached the artwork but not the dictionary would fail it, and the
    // resize would report `RecordedNotPainted` instead of redrawing.
    let mut s = session();
    s.add_push_button(
        &NewPushButton::new(0, "Go", rect(), "Submit")
            .declining_tooltip()
            .with_background(MkColor::Rgb(0.1, 0.3, 0.7)),
    )
    .unwrap();

    let out = s
        .edit_widget(
            "Go",
            0,
            &WidgetEdit::new().with_rect(Rect {
                llx: 20.0,
                lly: 100.0,
                urx: 160.0,
                ury: 140.0,
            }),
        )
        .unwrap();
    assert_eq!(
        out.appearance,
        AppearanceOutcome::Regenerated,
        "pdfcer must still recognise its own artwork"
    );

    let art = ap_text(&s, "Go");
    assert!(
        art.contains("0.1 0.3 0.7 rg"),
        "and redraw it in the colour it was created in: {art}"
    );
}

#[test]
fn a_creation_colour_survives_save_and_reopen() {
    let mut s = session();
    s.add_check_box(
        &NewCheckBox::new(0, "Agree", rect())
            .declining_tooltip()
            .with_background(MkColor::Rgb(0.2, 0.4, 0.9))
            .with_border_color(MkColor::Rgb(1.0, 0.0, 0.0)),
    )
    .unwrap();
    let (bytes, _) = s
        .to_full_bytes(&pdfcer_core::writer::SaveOptions::default())
        .unwrap();

    let reopened = EditSession::new(Document::from_bytes(bytes).unwrap());
    let art = ap_text(&reopened, "Agree");
    assert!(art.contains("0.2 0.4 0.9 rg"), "{art}");
    assert!(art.contains("1 0 0 RG"), "{art}");
    let w = &field_named(&reopened, "Agree").widgets[0];
    assert_eq!(w.background, Some(MkColor::Rgb(0.2, 0.4, 0.9)));
    assert_eq!(w.border_color, Some(MkColor::Rgb(1.0, 0.0, 0.0)));
}

#[test]
fn a_second_widget_merged_into_one_field_carries_its_own_colour() {
    // §12.7.3.2: two widgets sharing a name are two views of ONE field. The
    // LOOK is per widget, so a second placement may be a different colour —
    // and the colour has to reach the merge branch's dictionary, which is a
    // different code path from the create branch's.
    let mut s = session();
    s.add_text_field(
        &NewTextField::new(0, "Ref", rect())
            .declining_tooltip()
            .with_background(MkColor::Gray(0.9)),
    )
    .unwrap();
    let second = Rect {
        llx: 200.0,
        lly: 300.0,
        urx: 240.0,
        ury: 324.0,
    };
    let out = s
        .add_text_field(
            &NewTextField::new(0, "Ref", second)
                .declining_tooltip()
                .with_background(MkColor::Rgb(1.0, 0.9, 0.0)),
        )
        .unwrap();
    assert!(out.merged, "the second add attaches a widget, not a field");

    let field = field_named(&s, "Ref");
    assert_eq!(field.widgets.len(), 2);
    assert_eq!(field.widgets[0].background, Some(MkColor::Gray(0.9)));
    assert_eq!(
        field.widgets[1].background,
        Some(MkColor::Rgb(1.0, 0.9, 0.0))
    );
    assert!(ap_text_of(&s, "Ref", 1).contains("1 0.9 0 rg"));
}

// -------------------------------------------------------------------------
// Absent is no longer a one-way door (`Pass 308.3`, request `G021`)
// -------------------------------------------------------------------------
//
// `WidgetEdit` wrapped its two colour setters in one `Option` whose `None`
// already meant *this edit does not mention the key*, so `Some(MkColor::None)`
// was the empty array and **absent had no spelling at all**. Every transition
// back INTO absent was unreachable.
//
// On a push button that is a different RENDERING, not a different byte: absent
// means the plate grey, the empty array means no plate. An operator who chose
// *no background* could not get the plate back.

#[test]
fn a_push_button_stripped_of_its_plate_can_get_it_back() {
    let mut s = session();
    s.add_push_button(&NewPushButton::new(0, "Go", rect(), "Submit").declining_tooltip())
        .unwrap();
    assert!(
        ap_text(&s, "Go").contains("0.85 g"),
        "the plate to begin with"
    );

    // Table 189's empty array: no plate, positively stated.
    s.edit_widget("Go", 0, &WidgetEdit::new().with_background(MkColor::None))
        .unwrap();
    assert!(!ap_text(&s, "Go").contains("0.85 g"), "the plate is gone");
    assert_eq!(
        field_named(&s, "Go").widgets[0].background,
        Some(MkColor::None)
    );

    // And back — which had no spelling at all before this Pass.
    let out = s
        .edit_widget("Go", 0, &WidgetEdit::new().without_background())
        .unwrap();
    assert_eq!(out.appearance, AppearanceOutcome::Regenerated);
    assert!(
        ap_text(&s, "Go").contains("0.85 g"),
        "an ABSENT /BG is what 'the plate grey' means, so removing the key \
         restores it: {}",
        ap_text(&s, "Go")
    );
    assert_eq!(
        field_named(&s, "Go").widgets[0].background,
        None,
        "and the key is gone, not set to the constant — a widget pdfcer \
         never logically touched comes back byte-identical (R33)"
    );
}

#[test]
fn removing_the_last_mk_entry_removes_the_dictionary_with_it() {
    // A text field created with no colour carries no `/MK` at all. Colour it,
    // then take both colours away: the widget must be back where it started,
    // not left holding an empty dictionary it never had.
    let mut s = session();
    s.add_text_field(&NewTextField::new(0, "Name", rect()).declining_tooltip())
        .unwrap();
    assert!(!has_mk(&s, "Name"), "nothing to begin with");

    s.edit_widget(
        "Name",
        0,
        &WidgetEdit::new()
            .with_background(MkColor::Gray(0.95))
            .with_border_color(MkColor::Gray(0.0)),
    )
    .unwrap();
    assert!(has_mk(&s, "Name"));
    assert!(ap_text(&s, "Name").contains("0.95 g"));

    s.edit_widget(
        "Name",
        0,
        &WidgetEdit::new()
            .without_background()
            .without_border_color(),
    )
    .unwrap();
    assert!(
        !has_mk(&s, "Name"),
        "an /MK with nothing left in it is removed, not written empty"
    );
    let art = ap_text(&s, "Name");
    assert!(!art.contains("\nf\n"), "nothing is filled again: {art}");
    assert!(!art.contains("\nS\n"), "and nothing is stroked: {art}");
}

#[test]
fn a_removal_leaves_the_other_mk_entries_alone() {
    // `/MK` carries more than the two colours — a check box's tick style lives
    // in `/CA`, and a removal that replaced the dictionary would silently
    // delete it. Preserve-and-patch, asserted rather than assumed.
    let mut s = session();
    s.add_check_box(
        &NewCheckBox::new(0, "Agree", rect())
            .declining_tooltip()
            .with_background(MkColor::Gray(0.9)),
    )
    .unwrap();
    s.edit_widget("Agree", 0, &WidgetEdit::new().without_background())
        .unwrap();

    assert_eq!(field_named(&s, "Agree").widgets[0].background, None);
    assert!(
        has_mk(&s, "Agree"),
        "/MK survives, because /CA is still in it"
    );
    let g = s.graph();
    let id = field_named(&s, "Agree").widgets[0].id;
    let mk = g
        .resolved(id)
        .as_dict()
        .and_then(|d| d.get(b"MK"))
        .and_then(Object::as_dict)
        .cloned()
        .expect("an /MK");
    assert!(mk.get(b"CA").is_some(), "the tick style is untouched");
    assert!(mk.get(b"BG").is_none(), "and only /BG went");
}

#[test]
fn removing_a_colour_that_was_never_there_changes_nothing() {
    // Idempotent, and it must not invent an `/MK` on the way past.
    let mut s = session();
    s.add_text_field(&NewTextField::new(0, "Name", rect()).declining_tooltip())
        .unwrap();
    let before = ap_text(&s, "Name");

    s.edit_widget(
        "Name",
        0,
        &WidgetEdit::new()
            .without_background()
            .without_border_color(),
    )
    .unwrap();

    assert!(!has_mk(&s, "Name"), "no /MK was conjured to hold nothing");
    assert_eq!(ap_text(&s, "Name"), before, "and the artwork is unchanged");
}

#[test]
fn the_three_mk_states_are_all_reachable_in_both_directions() {
    // The table `G021` drew: every cell that said "no route" now has one.
    let mut s = session();
    s.add_push_button(&NewPushButton::new(0, "Go", rect(), "Submit").declining_tooltip())
        .unwrap();

    let states = |s: &EditSession| field_named(s, "Go").widgets[0].background;

    // absent -> colour -> empty -> absent -> empty -> colour -> absent
    assert_eq!(
        states(&s),
        Some(MkColor::Gray(0.85)),
        "created WITH a plate"
    );
    s.edit_widget("Go", 0, &WidgetEdit::new().without_background())
        .unwrap();
    assert_eq!(states(&s), None);
    s.edit_widget(
        "Go",
        0,
        &WidgetEdit::new().with_background(MkColor::Rgb(0.0, 0.0, 1.0)),
    )
    .unwrap();
    assert_eq!(states(&s), Some(MkColor::Rgb(0.0, 0.0, 1.0)));
    s.edit_widget("Go", 0, &WidgetEdit::new().with_background(MkColor::None))
        .unwrap();
    assert_eq!(states(&s), Some(MkColor::None));
    s.edit_widget("Go", 0, &WidgetEdit::new().without_background())
        .unwrap();
    assert_eq!(states(&s), None, "the transition that had no route");
}

#[test]
fn the_edit_enum_resolves_a_removal_to_no_colour() {
    // The one place the three states are named. `resolved()` is what both the
    // regenerator and the dictionary writer read, so a drift here would show
    // up as artwork and dictionary disagreeing — the `Pass 308.0` failure.
    assert_eq!(
        MkColorEdit::Set(MkColor::Gray(0.5)).resolved(),
        Some(MkColor::Gray(0.5))
    );
    assert_eq!(MkColorEdit::Remove.resolved(), None);
}
