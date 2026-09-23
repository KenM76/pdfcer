//! **`/MK` `/BG` and `/BC` are BAKED INTO THE APPEARANCE, not merely
//! recorded** (`Pass 308.0`, request `G020`).
//!
//! ## The defect these tests exist because of
//!
//! `Widget::background` read `/MK` `/BG`. `WidgetEdit::with_background` wrote
//! it. The CLI shipped `--background`. **Nothing painted it.** No appearance
//! builder took a colour, and `edit_widget`'s `needs_regen` did not include
//! either colour, so a colour-only edit returned `appearance_regenerated:
//! false` and changed the dictionary and nothing else.
//!
//! That is not a cosmetic gap, and R43 is the reason: **pdfcer paints the
//! baked `/AP` and never reconstructs an appearance from `/MK` at display
//! time.** `/MK` is the *appearance characteristics* dictionary — a record of
//! what artwork should look like, for a producer regenerating it. A widget
//! whose `/BG` says blue and whose `/AP` draws grey renders **grey**, here
//! and in every other conforming reader. So writing `/BG` and stopping is
//! writing a value nothing acts on.
//!
//! `pdfcer-gui` reported it from the sharp end: its on-canvas field editor
//! already tints from `Widget::background`, so shipping the swatch would have
//! made a screenshot of the editing canvas differ from a screenshot of the
//! same document saved and reopened — **the one-line test project rule 4
//! forbids failing**.
//!
//! ## What is asserted, and why it is the bytes
//!
//! **The operators in the appearance stream.** A model assertion — "the
//! widget's background is blue" — passes on the broken build, because the
//! broken build wrote `/MK` `/BG` correctly. It was only ever the artwork
//! that did not follow. `R159`: a defect that lives in the bytes is asserted
//! in the bytes.
//!
//! ## The half that must NOT change is asserted too
//!
//! Every builder's default has to stay byte-identical, and two of them are
//! traps in opposite directions:
//!
//! * **A text field draws no box at all.** If an absent `/BG` produced a
//!   white rectangle, every text field in every document pdfcer touches
//!   would silently gain one.
//! * **A push button's default is NOT "nothing"** — it is the plate grey,
//!   which `add_push_button` also writes into `/MK` `/BG`. Those agreed by
//!   construction before this Pass and now agree because the artwork reads
//!   the dictionary. A default of "nothing" would erase every plate.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{
    AppearanceOutcome, EditSession, NewCheckBox, NewPushButton, NewRadioButton, NewTextField,
    WidgetEdit,
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

/// Every appearance stream reachable from this widget's `/AP` `/N`, as text.
///
/// One reader for both `/N` shapes — a stream (text, choice, push button) and
/// a state sub-dictionary (check box, radio) — because a reader that
/// understood only one would silently assert nothing about the other.
/// Streams come back in `/N` key order for the dictionary shape, which for a
/// pdfcer-authored button is `Off` then the on-state.
fn ap_streams(s: &EditSession, name: &str) -> Vec<String> {
    let g = s.graph();
    let field = field_named(s, name);
    let dict = g
        .resolved(field.widgets[0].id)
        .as_dict()
        .cloned()
        .expect("widget dict");
    let Some(Object::Dict(ap)) = dict.get(b"AP").map(|o| g.resolve(o).clone()) else {
        return Vec::new();
    };
    let Some(n) = ap.get(b"N") else {
        return Vec::new();
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
        .collect()
}

/// The `/AP` `/N` streams joined — for assertions that do not care which
/// state an operator landed in.
fn ap_text(s: &EditSession, name: &str) -> String {
    ap_streams(s, name).join("\n")
}

// -------------------------------------------------------------------------
// The defect: a colour reaches the artwork at all
// -------------------------------------------------------------------------

#[test]
fn a_background_colour_is_painted_into_a_check_box() {
    let mut s = session();
    s.add_check_box(&NewCheckBox::new(0, "Agree", rect()).declining_tooltip())
        .unwrap();
    assert!(
        !ap_text(&s, "Agree").contains(" rg"),
        "a freshly created check box has no background at all"
    );

    let out = s
        .edit_widget(
            "Agree",
            0,
            &WidgetEdit::new().with_background(MkColor::Rgb(0.2, 0.4, 0.9)),
        )
        .unwrap();

    // The whole point: a colour-only edit now REDRAWS. Before this Pass it
    // fell to `else { false }` and the dictionary changed alone.
    assert!(out.appearance_regenerated);
    assert_eq!(out.appearance, AppearanceOutcome::Regenerated);

    let art = ap_text(&s, "Agree");
    assert!(
        art.contains("0.2 0.4 0.9 rg"),
        "the fill operator must be in the stream: {art}"
    );
    // And the dictionary still says it too — the two halves agree.
    let w = &field_named(&s, "Agree").widgets[0];
    assert_eq!(w.background, Some(MkColor::Rgb(0.2, 0.4, 0.9)));
}

#[test]
fn a_border_colour_is_painted_into_a_check_box() {
    let mut s = session();
    s.add_check_box(&NewCheckBox::new(0, "Agree", rect()).declining_tooltip())
        .unwrap();
    assert!(
        ap_text(&s, "Agree").contains("0 G"),
        "the default border is black"
    );

    s.edit_widget(
        "Agree",
        0,
        &WidgetEdit::new().with_border_color(MkColor::Rgb(1.0, 0.0, 0.0)),
    )
    .unwrap();
    let art = ap_text(&s, "Agree");
    assert!(art.contains("1 0 0 RG"), "{art}");
    assert!(!art.contains("0 G"), "and the black stroke is gone: {art}");
}

#[test]
fn a_radio_buttons_dot_takes_the_border_colour_not_the_background() {
    // `/MK` has no third colour meaning "the mark". A dot drawn in the
    // BACKGROUND colour would be invisible against the disc it sits on, so
    // pdfcer reads `/BC` as the control's ink — a choice, and one the
    // builder's doc comment states.
    let mut s = session();
    s.add_radio_button(&NewRadioButton::new(0, "Choice", rect(), "A").declining_tooltip())
        .unwrap();
    s.edit_widget(
        "Choice",
        0,
        &WidgetEdit::new()
            .with_background(MkColor::Gray(0.9))
            .with_border_color(MkColor::Rgb(0.0, 0.5, 0.0)),
    )
    .unwrap();

    let states = ap_streams(&s, "Choice");
    assert_eq!(states.len(), 2, "off and on");
    let on = states
        .iter()
        .find(|s| s.matches(" f\n").count() >= 2 || s.contains("0 0.5 0 rg"))
        .expect("the on state fills a dot as well as the disc");
    assert!(
        on.contains("0.9 g"),
        "the disc is filled in the background colour: {on}"
    );
    assert!(
        on.contains("0 0.5 0 rg"),
        "and the dot in the border colour: {on}"
    );
}

// -------------------------------------------------------------------------
// The half that must not change
// -------------------------------------------------------------------------

#[test]
fn a_text_field_still_draws_no_box_when_it_states_no_colour() {
    // The trap in the "give the builders a colour" direction. This builder
    // has NEVER painted a background or a frame, and an absent /BG that
    // produced a white rectangle would repaint every text field in every
    // document pdfcer touches.
    let mut s = session();
    s.add_text_field(&NewTextField::new(0, "Name", rect()).declining_tooltip())
        .unwrap();
    let art = ap_text(&s, "Name");
    // There IS one `re` — the variable-text generator's own clip, `re W n`.
    // What must be absent is anything PAINTED: no fill, no stroke.
    assert!(art.contains("re\nW\nn"), "the text clip is expected: {art}");
    assert!(!art.contains("\nf\n"), "nothing is filled: {art}");
    assert!(!art.contains("\nS\n"), "and nothing is stroked: {art}");
}

#[test]
fn a_text_field_gains_a_box_only_when_it_asks_for_one() {
    let mut s = session();
    s.add_text_field(&NewTextField::new(0, "Name", rect()).declining_tooltip())
        .unwrap();
    s.edit_widget(
        "Name",
        0,
        &WidgetEdit::new()
            .with_background(MkColor::Gray(0.95))
            .with_border_color(MkColor::Gray(0.0)),
    )
    .unwrap();
    let art = ap_text(&s, "Name");
    assert!(art.contains("0.95 g"), "{art}");
    assert!(art.contains("0 G"), "{art}");
    // The box is drawn BEFORE the text object. §8.2 Table 51 does not admit
    // `q`/`Q` inside `BT`…`ET`, and the variable-text generator clips, so
    // anything painted afterwards would be inside that clip or malformed.
    let box_at = art.find("0.95 g").unwrap();
    if let Some(bt_at) = art.find("BT") {
        assert!(box_at < bt_at, "the box precedes the text object: {art}");
    }
}

#[test]
fn a_push_buttons_default_plate_is_the_grey_it_has_always_been() {
    // The trap in the other direction. This is the ONE builder whose
    // default background is not "nothing", and `add_push_button` writes that
    // same constant into `/MK` `/BG`. A default of "nothing" would erase
    // every plate pdfcer has ever drawn.
    let mut s = session();
    s.add_push_button(&NewPushButton::new(0, "Go", rect(), "Submit").declining_tooltip())
        .unwrap();
    let art = ap_text(&s, "Go");
    assert!(art.contains("0.85 g"), "the plate grey: {art}");
    assert_eq!(
        field_named(&s, "Go").widgets[0].background,
        Some(MkColor::Gray(0.85)),
        "and the dictionary says the same thing"
    );
}

#[test]
fn a_push_button_can_be_given_no_plate_at_all() {
    // Table 189's EMPTY ARRAY is "no colour", and it is not the same as the
    // key being absent — which is exactly why this is expressible. An absent
    // key falls back to the plate grey; an empty one says, positively, that
    // there is no plate.
    let mut s = session();
    s.add_push_button(&NewPushButton::new(0, "Go", rect(), "Submit").declining_tooltip())
        .unwrap();
    s.edit_widget("Go", 0, &WidgetEdit::new().with_background(MkColor::None))
        .unwrap();
    let art = ap_text(&s, "Go");
    assert!(
        !art.contains("0.85 g"),
        "the plate is gone, not recoloured: {art}"
    );
    assert!(
        art.starts_with("0 G\n"),
        "the keyline survives, and is now the FIRST thing drawn — /BS /W is \
         what removes a border, not an empty /BC: {art}"
    );
}

// -------------------------------------------------------------------------
// CMYK is painted, never converted
// -------------------------------------------------------------------------

#[test]
fn a_cmyk_background_is_painted_as_cmyk() {
    // pdfcer owns no rendering intent for a widget's chrome, so converting
    // to RGB here would be the substitution `Widget::border` refuses
    // elsewhere. The operator's separation is what the file says; it is
    // therefore what the stream says.
    let mut s = session();
    s.add_check_box(&NewCheckBox::new(0, "Agree", rect()).declining_tooltip())
        .unwrap();
    s.edit_widget(
        "Agree",
        0,
        &WidgetEdit::new().with_background(MkColor::Cmyk(0.1, 0.2, 0.3, 0.4)),
    )
    .unwrap();
    let art = ap_text(&s, "Agree");
    assert!(art.contains("0.1 0.2 0.3 0.4 k"), "{art}");
    assert!(
        !art.contains(" rg"),
        "and nothing was converted on the way: {art}"
    );
}

// -------------------------------------------------------------------------
// The outcome can say "recorded, not painted"
// -------------------------------------------------------------------------

#[test]
fn an_edit_that_changes_nothing_drawable_says_so() {
    let mut s = session();
    s.add_check_box(&NewCheckBox::new(0, "Agree", rect()).declining_tooltip())
        .unwrap();
    // A pure move. The artwork is correct where it stands.
    let moved = Rect {
        llx: 200.0,
        lly: 300.0,
        urx: 240.0,
        ury: 324.0,
    };
    let out = s
        .edit_widget("Agree", 0, &WidgetEdit::new().with_rect(moved))
        .unwrap();
    assert_eq!(out.appearance, AppearanceOutcome::NotNeeded);
    assert!(!out.appearance.needs_disclosure());
}

#[test]
fn the_three_outcome_states_are_distinguishable() {
    // Before `Pass 308.1` the two older fields encoded these three states as
    // `false`+`None`, `true`+`None` and `false`+`Some` — and the caller had
    // to know that the first combination meant two different things.
    let mut s = session();
    s.add_check_box(&NewCheckBox::new(0, "Agree", rect()).declining_tooltip())
        .unwrap();

    let redrawn = s
        .edit_widget(
            "Agree",
            0,
            &WidgetEdit::new().with_background(MkColor::Gray(0.5)),
        )
        .unwrap();
    assert_eq!(redrawn.appearance, AppearanceOutcome::Regenerated);
    assert!(redrawn.appearance_regenerated);
    assert!(redrawn.appearance_stale.is_none());

    let untouched = s
        .edit_widget(
            "Agree",
            0,
            &WidgetEdit::new().with_visibility(pdfcer_core::edit::Visibility::Hidden),
        )
        .unwrap();
    assert_eq!(untouched.appearance, AppearanceOutcome::NotNeeded);
    assert!(!untouched.appearance_regenerated);
    assert!(untouched.appearance_stale.is_none());

    // The two agree with each other, always — the enum is derived from them
    // rather than tracked separately, so they cannot drift.
    for out in [&redrawn, &untouched] {
        assert_eq!(
            out.appearance_regenerated,
            out.appearance == AppearanceOutcome::Regenerated
        );
        assert_eq!(
            out.appearance_stale.is_some(),
            out.appearance.needs_disclosure()
        );
    }
}

// -------------------------------------------------------------------------
// It survives the round trip
// -------------------------------------------------------------------------

#[test]
fn a_coloured_widget_survives_save_and_reopen() {
    let mut s = session();
    s.add_check_box(&NewCheckBox::new(0, "Agree", rect()).declining_tooltip())
        .unwrap();
    s.edit_widget(
        "Agree",
        0,
        &WidgetEdit::new()
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
