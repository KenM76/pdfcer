//! Copying a form field and planting it somewhere else (`Pass 167.0`).
//!
//! ## What these tests are really checking
//!
//! A field paste is **five coordinated writes**: the field dictionary, its
//! widget(s), the page's `/Annots` entry, the `/AcroForm` `/Fields`
//! registration, and — when the clip carries one — a `/DR` font entry the
//! field's `/DA` names. Any four without the fifth produce a document that is
//! broken in a way nothing reports: registered-but-not-annotated is invisible,
//! annotated-but-not-registered is not a field at all, and a `/DA` naming a
//! font the `/DR` lacks is a field that looks right here and re-renders wrong
//! in every other viewer.
//!
//! So the load-bearing assertion throughout is **"`parse_acroform` reads the
//! SAVED BYTES back as the field we meant"** — through the same parser the
//! fill path, the CLI and the GUI use, after a real save and reload rather
//! than against the live session's overlay.
//!
//! ## Why the fixture matters more here than anywhere else in forms
//!
//! Field *creation* chooses every value itself, so a fixture carrying the
//! authoring defaults can verify it. A field *copy* is judged entirely on
//! values pdfcer did **not** choose — and every one of them is invisible when
//! the fixture's value happens to equal the default. A `/DA` of
//! `/Helv 0 Tf 0 g` cannot show that font, size and colour travelled: it is
//! exactly what a re-author would have written anyway.
//!
//! `rich-field-form.pdf` therefore has **nothing** at its default:
//! `/TB 14 Tf 0 0 1 rg`, centred, 12-character limit, blue border, cream
//! background, dashed 2 pt, `DoNotSpellCheck` set, `/V` and `/DV` different,
//! a calculate action, and a `/DA` naming a font resource no destination has.
//! Tests written against a default-valued fixture would pass against an
//! implementation that carried nothing.
//!
//! ## The two chords must not become each other
//!
//! Half of this file exists for one reason: `Ctrl+V` (a new, independent
//! field) and `Ctrl+Shift+V` (another widget of the same field) are different
//! gestures, and the difference is **invisible on the page**. Two fields that
//! look identical differ only in whether typing in one shows in the other. So
//! each policy's refusal of the other's situation is asserted by error
//! variant, not by outcome shape.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::formclip::{FieldClip, FieldPastePolicy, PasteTooltip};
use pdfcer_core::forms::{self, ButtonKind, FieldType};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{Dict, Object};
use pdfcer_core::page_tree::Rect;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(name)
}

fn session(rel: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(rel)).expect("load fixture"))
}

fn rect() -> Rect {
    Rect {
        llx: 200.0,
        lly: 60.0,
        urx: 360.0,
        ury: 84.0,
    }
}

fn new_field(name: &str) -> FieldPastePolicy {
    FieldPastePolicy::NewField {
        name: name.to_owned(),
        tooltip: PasteTooltip::Declined,
        copy_value: false,
        copy_actions: false,
    }
}

fn field_named(s: &EditSession, name: &str) -> Option<forms::Field> {
    forms::parse_acroform(&s.graph())?
        .fields
        .into_iter()
        .find(|f| f.fully_qualified_name == name)
}

/// Save and reload, so every assertion below is about BYTES rather than about
/// a session's in-memory overlay.
fn reload(session: &EditSession) -> EditSession {
    let (bytes, _) = session
        .to_full_bytes(&Default::default())
        .expect("full rewrite");
    EditSession::new(Document::from_bytes(bytes).expect("reload"))
}

/// The `/AcroForm /DR /Font` dictionary of a saved document.
fn dr_fonts(s: &EditSession) -> Dict {
    let graph = s.graph();
    let catalog = graph.catalog_dict().expect("catalog");
    let acroform = graph
        .resolve(catalog.get(b"AcroForm").expect("/AcroForm"))
        .as_dict()
        .expect("/AcroForm is a dict")
        .clone();
    let dr = graph
        .resolve(acroform.get(b"DR").expect("/DR"))
        .as_dict()
        .expect("/DR is a dict")
        .clone();
    graph
        .resolve(dr.get(b"Font").expect("/DR /Font"))
        .as_dict()
        .expect("/DR /Font is a dict")
        .clone()
}

// ---------------------------------------------------------------------------
// The headline behaviour: the properties a re-author would lose
// ---------------------------------------------------------------------------

/// `/DA`, `/Q` and `/MaxLen` — the three the consuming shell named as the ones
/// an operator *sees*.
///
/// "The copy looks wrong" is a 14 pt blue centred field pasting back as a
/// left-aligned black authoring default.
#[test]
fn a_pasted_field_keeps_the_appearance_string_quadding_and_length_limit() {
    let mut s = session("forms/rich-field-form.pdf");
    let clip = s.copy_field("TitleBlock.Revision").expect("copy");
    assert_eq!(clip.field_type(), Some(FieldType::Text));
    assert_eq!(clip.widget_count(), 1);
    assert_eq!(clip.source_name(), "TitleBlock.Revision");

    s.paste_field(&clip, 0, rect(), &new_field("TitleBlock.Rev2"))
        .expect("paste");
    let reloaded = reload(&s);
    let pasted = field_named(&reloaded, "TitleBlock.Rev2").expect("pasted field");

    assert_eq!(
        pasted.default_appearance.as_deref(),
        Some(b"/TB 14 Tf 0 0 1 rg".as_slice()),
        "the /DA -- font, size and colour -- travelled verbatim",
    );
    assert_eq!(
        pasted.quadding,
        pdfcer_core::vartext::Quadding::Center,
        "the /Q travelled, so a centred title-block field does not paste left-aligned",
    );
    assert_eq!(pasted.max_len, Some(12), "the /MaxLen travelled");
    assert_eq!(
        pasted.widgets.first().and_then(|w| w.rect),
        Some(rect()),
        "a single-widget paste uses the rectangle the operator drew, verbatim",
    );
    assert!(
        pasted.merged,
        "one widget means the merged Shape A form (SS12.5.6.19) -- the same \
         shape every add_*_field verb writes, so a pasted field is \
         indistinguishable from an authored one",
    );
    // A copy is not a move.
    assert!(field_named(&reloaded, "TitleBlock.Revision").is_some());
}

/// The flags no `New*Field` spec can express arrive only by being carried.
///
/// `DoNotSpellCheck` (bit 23) is not on any authoring spec, so a
/// read-and-re-author path structurally cannot reproduce it — which is the
/// shell's own point about the fidelity table it was maintaining by hand.
#[test]
fn a_pasted_field_keeps_flags_no_authoring_spec_can_express() {
    let mut s = session("forms/rich-field-form.pdf");
    let clip = s.copy_field("TitleBlock.Revision").expect("copy");
    s.paste_field(&clip, 0, rect(), &new_field("Flags"))
        .expect("paste");
    let reloaded = reload(&s);
    let pasted = field_named(&reloaded, "Flags").expect("field");
    assert!(
        pasted.flags.has(forms::FieldFlags::DO_NOT_SPELL_CHECK),
        "DoNotSpellCheck (/Ff bit 23) survived; flags = {:#x}",
        pasted.flags.0,
    );
}

/// The `/MK` colours the shell reported as pasting black, and the `/BS` the
/// authoring path can only express as two of its five styles.
#[test]
fn a_pasted_widget_keeps_its_border_colour_background_and_style() {
    let mut s = session("forms/rich-field-form.pdf");
    let clip = s.copy_field("TitleBlock.Revision").expect("copy");
    let outcome = s
        .paste_field(&clip, 0, rect(), &new_field("Blue"))
        .expect("paste");
    let reloaded = reload(&s);
    let id = outcome.widget_ids.first().copied().expect("a widget");
    let graph = reloaded.graph();
    let widget = graph.resolved(id).as_dict().expect("widget dict");

    let mk = graph
        .resolve(widget.get(b"MK").expect("/MK survived"))
        .as_dict()
        .expect("/MK is a dict");
    assert_eq!(
        graph.resolve(mk.get(b"BC").expect("/BC survived")),
        &Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(1)
        ]),
        "a blue-bordered field must not paste black",
    );
    assert!(
        mk.get(b"BG").is_some(),
        "the background colour travelled too -- nothing in pdfcer AUTHORS a /BG, \
         so it can only be here by having been carried",
    );
    let bs = graph
        .resolve(widget.get(b"BS").expect("/BS survived"))
        .as_dict()
        .expect("/BS is a dict");
    assert_eq!(
        graph.resolve(bs.get(b"W").expect("/W")).as_number(),
        Some(2.0),
        "the 2pt width travelled, not the 1pt default",
    );
    assert!(
        widget.get(b"AP").is_some(),
        "the baked appearance travelled",
    );
}

/// `/DV` travels even when `/V` does not — Reset must restore the right thing.
#[test]
fn the_default_value_travels_even_when_the_value_does_not() {
    let mut s = session("forms/rich-field-form.pdf");
    let clip = s.copy_field("TitleBlock.Revision").expect("copy");
    assert!(clip.carries_value(), "precondition: the source has a /V");

    let outcome = s
        .paste_field(&clip, 0, rect(), &new_field("Empty"))
        .expect("paste");
    assert!(
        outcome
            .disclosures
            .iter()
            .any(|d| d.contains("VALUE was not carried")),
        "{:?}",
        outcome.disclosures,
    );
    let reloaded = reload(&s);
    let pasted = field_named(&reloaded, "Empty").expect("field");
    assert!(
        !pasted.value.is_present(),
        "a value is CONTENT: a copied field does not arrive pre-filled",
    );
    assert!(
        pasted.default_value.is_present(),
        "but /DV is the RESET TARGET, not content -- dropping it would make \
         Reset Form restore the wrong thing, silently",
    );
}

#[test]
fn the_value_is_carried_when_asked() {
    let mut s = session("forms/rich-field-form.pdf");
    let clip = s.copy_field("TitleBlock.Revision").expect("copy");
    let with = FieldPastePolicy::NewField {
        name: "Filled".to_owned(),
        tooltip: PasteTooltip::Declined,
        copy_value: true,
        copy_actions: false,
    };
    s.paste_field(&clip, 0, rect(), &with).expect("paste");
    let reloaded = reload(&s);
    assert_eq!(
        field_named(&reloaded, "Filled")
            .expect("field")
            .value
            .display_text(),
        "C",
    );
}

// ---------------------------------------------------------------------------
// The /DA font — what makes the clip beat six property setters
// ---------------------------------------------------------------------------

/// Copy into a document that has never heard of the `/DA`'s font, and the
/// font arrives with it.
///
/// §12.7.3.3 makes the `/DA`'s `Tf` name resolvable in `/AcroForm /DR /Font`.
/// Carrying the `/DA` without the font it names produces a field pdfcer can
/// still render — its `/AP` has its own resources — and that every other
/// viewer regenerating from `/DA` cannot. That is a document which works here
/// and nowhere else, which is worse than a visible failure.
#[test]
fn the_font_the_appearance_string_names_travels_with_the_field() {
    let source = session("forms/rich-field-form.pdf");
    let clip = source.copy_field("TitleBlock.Revision").expect("copy");
    assert_eq!(
        clip.carried_font(),
        Some(b"TB".as_slice()),
        "the clip knows which font resource it is carrying",
    );

    let mut destination = session("hello.pdf");
    assert!(
        forms::parse_acroform(&destination.graph()).is_none(),
        "precondition: the destination has no interactive form at all",
    );
    destination
        .paste_field(&clip, 0, rect(), &new_field("Imported"))
        .expect("paste");
    let reloaded = reload(&destination);

    let field = field_named(&reloaded, "Imported").expect("the field parses back");
    let da = field.default_appearance.expect("a /DA");
    let parsed = pdfcer_core::vartext::parse_default_appearance(&da).expect("well-formed /DA");
    assert_eq!(parsed.font_name, b"TB".to_vec());
    let fonts = dr_fonts(&reloaded);
    let installed = reloaded
        .graph()
        .resolve(fonts.get(b"TB").expect("/DR /Font /TB was installed"))
        .as_dict()
        .expect("a font dict")
        .clone();
    assert_eq!(
        installed.get(b"BaseFont"),
        Some(&Object::Name(pdfcer_core::object::Name::from(
            b"Helvetica-Bold"
        ))),
        "and it is the SOURCE's font, not a substitute",
    );
}

/// The destination already uses that resource name for a **different** font.
///
/// Overwriting it would silently restyle every field in the destination that
/// names it. Refusing would break a paste for a reason the operator cannot
/// act on. So the carried font gets a free name and the pasted field's `/DA`
/// is rewritten to match — the only outcome in which both documents keep the
/// look they had.
#[test]
fn a_colliding_font_resource_name_is_renamed_and_the_appearance_string_rewritten() {
    let source = session("forms/rich-field-form.pdf");
    let clip = source.copy_field("TitleBlock.Revision").expect("copy");

    let mut destination = session("forms/rival-font-form.pdf");
    let before = dr_fonts(&destination);
    let rival = destination
        .graph()
        .resolve(before.get(b"TB").expect("precondition: /TB exists here"))
        .as_dict()
        .expect("dict")
        .get(b"BaseFont")
        .cloned();
    assert_eq!(
        rival,
        Some(Object::Name(pdfcer_core::object::Name::from(b"Courier"))),
        "precondition: the destination's /TB is a DIFFERENT font",
    );

    let outcome = destination
        .paste_field(&clip, 0, rect(), &new_field("Imported"))
        .expect("paste");
    assert!(
        outcome
            .disclosures
            .iter()
            .any(|d| d.contains("already uses the font resource")),
        "the rename is disclosed: {:?}",
        outcome.disclosures,
    );

    let reloaded = reload(&destination);
    let fonts = dr_fonts(&reloaded);
    assert_eq!(
        reloaded
            .graph()
            .resolve(fonts.get(b"TB").expect("/TB still here"))
            .as_dict()
            .and_then(|d| d.get(b"BaseFont"))
            .cloned(),
        Some(Object::Name(pdfcer_core::object::Name::from(b"Courier"))),
        "the destination's OWN /TB was not clobbered",
    );
    let fresh = fonts
        .get(b"TB_1")
        .expect("the carried font was installed under a free name");
    assert_eq!(
        reloaded
            .graph()
            .resolve(fresh)
            .as_dict()
            .and_then(|d| d.get(b"BaseFont"))
            .cloned(),
        Some(Object::Name(pdfcer_core::object::Name::from(
            b"Helvetica-Bold"
        ))),
    );
    let field = field_named(&reloaded, "Imported").expect("field");
    assert_eq!(
        field.default_appearance.as_deref(),
        Some(b"/TB_1 14 Tf 0 0 1 rg".as_slice()),
        "and the /DA was rewritten to name it -- a rename without the rewrite \
         would leave the field pointing at Courier",
    );
}

// ---------------------------------------------------------------------------
// The two chords, and their refusals
// ---------------------------------------------------------------------------

/// `Ctrl+V` onto a name in use is REFUSED, never auto-suffixed.
///
/// An engine-invented `Revision_2` is a name nobody chose, and the shell can
/// show the candidate before the press.
#[test]
fn pasting_as_a_new_field_refuses_a_name_that_is_taken() {
    let mut s = session("forms/rich-field-form.pdf");
    let clip = s.copy_field("TitleBlock.Revision").expect("copy");
    let err = s
        .paste_field(&clip, 0, rect(), &new_field("TitleBlock.Revision"))
        .expect_err("a taken name must be refused");
    assert!(
        matches!(err, EditError::FieldNameTaken { ref name } if name == "TitleBlock.Revision"),
        "got {err:?}",
    );
}

/// `Ctrl+Shift+V` adds a second VIEW of one field: one field, two widgets,
/// one value.
#[test]
fn pasting_as_an_additional_widget_adds_a_view_not_a_field() {
    let mut s = session("forms/rich-field-form.pdf");
    let before = forms::parse_acroform(&s.graph())
        .expect("form")
        .fields
        .len();
    let clip = s.copy_field("TitleBlock.Revision").expect("copy");
    let outcome = s
        .paste_field(
            &clip,
            0,
            rect(),
            &FieldPastePolicy::AdditionalWidget {
                existing: "TitleBlock.Revision".to_owned(),
            },
        )
        .expect("paste");
    assert!(!outcome.created, "no new field was created");
    assert!(
        outcome.disclosures.iter().any(|d| d.contains("SAME field")),
        "{:?}",
        outcome.disclosures,
    );

    let reloaded = reload(&s);
    let form = forms::parse_acroform(&reloaded.graph()).expect("form");
    assert_eq!(form.fields.len(), before, "the field COUNT did not change");
    let field = field_named(&reloaded, "TitleBlock.Revision").expect("field");
    assert_eq!(
        field.widgets.len(),
        2,
        "the field now has two views of itself",
    );
    assert!(
        !field.merged,
        "Shape A was promoted to Shape B: Table 220 permits the merged form \
         only while there is exactly one widget",
    );
    assert_eq!(
        field.value.display_text(),
        "C",
        "and the value is untouched -- it IS the same field",
    );
}

/// `Ctrl+Shift+V` naming a field this document does not have is REFUSED — it
/// never falls back to creating one.
///
/// This is the failure the two-policy split exists to prevent: a silent
/// fallback gives an independent field where the operator asked for a linked
/// one, and nothing on screen says so until somebody types in one and the
/// other does not follow.
#[test]
fn pasting_as_an_additional_widget_refuses_when_the_field_is_not_here() {
    let source = session("forms/rich-field-form.pdf");
    let clip = source.copy_field("TitleBlock.Revision").expect("copy");
    let mut destination = session("hello.pdf");
    let err = destination
        .paste_field(
            &clip,
            0,
            rect(),
            &FieldPastePolicy::AdditionalWidget {
                existing: "TitleBlock.Revision".to_owned(),
            },
        )
        .expect_err("a field that is not here has no view to add");
    assert!(
        matches!(err, EditError::FieldNotFound { ref name } if name == "TitleBlock.Revision"),
        "got {err:?}",
    );
}

/// A clip of one type onto a name held by another is refused — Acrobat's own
/// behaviour at this junction, and for the spec's reason: §12.7.3.2 makes two
/// same-FQN nodes representations of ONE field, and one field has one type.
#[test]
fn pasting_over_a_name_held_by_a_different_field_type_is_refused() {
    let source = session("forms/rich-field-form.pdf");
    let clip = source.copy_field("TitleBlock.Revision").expect("copy");
    let mut destination = session("forms/demo-form.pdf");
    let err = destination
        .paste_field(
            &clip,
            0,
            rect(),
            &FieldPastePolicy::AdditionalWidget {
                existing: "Subscribe".to_owned(),
            },
        )
        .expect_err("a text field is not a view of a check box");
    assert!(matches!(err, EditError::FieldAuthoring(_)), "got {err:?}",);
}

// ---------------------------------------------------------------------------
// Actions and the calculation order
// ---------------------------------------------------------------------------

/// A carried calculate action obliges the destination to grow `/CO`
/// (§12.7.2 Table 218), and a dropped one is said out loud.
#[test]
fn a_carried_calculation_reaches_the_calculation_order_and_a_dropped_one_is_disclosed() {
    let source = session("forms/rich-field-form.pdf");
    let clip = source.copy_field("TitleBlock.Revision").expect("copy");
    assert!(clip.carries_actions(), "the source carries /AA");
    assert!(clip.carries_calculation(), "and one of them is /AA /C");

    let mut inert = session("forms/rich-field-form.pdf");
    let dropped = inert
        .paste_field(&clip, 0, rect(), &new_field("Inert"))
        .expect("paste without actions");
    assert!(
        dropped
            .disclosures
            .iter()
            .any(|d| d.contains("/AA") && d.contains("NOT carried")),
        "an invisible loss must be said out loud: {:?}",
        dropped.disclosures,
    );
    let reloaded = reload(&inert);
    assert!(
        !field_named(&reloaded, "Inert")
            .expect("field")
            .has_additional_actions,
    );
    assert_eq!(
        forms::parse_acroform(&reloaded.graph())
            .expect("form")
            .calc_order
            .len(),
        1,
        "and /CO did not grow for a field that carries no calculation",
    );

    let mut live = session("forms/rich-field-form.pdf");
    let carried = FieldPastePolicy::NewField {
        name: "Live".to_owned(),
        tooltip: PasteTooltip::Declined,
        copy_value: false,
        copy_actions: true,
    };
    let outcome = live.paste_field(&clip, 0, rect(), &carried).expect("paste");
    assert!(
        outcome
            .disclosures
            .iter()
            .any(|d| d.contains("calculation order")),
        "{:?}",
        outcome.disclosures,
    );
    let reloaded = reload(&live);
    let form = forms::parse_acroform(&reloaded.graph()).expect("form");
    assert_eq!(
        form.calc_order.len(),
        2,
        "Table 218 makes /CO REQUIRED once any field carries an /AA /C, so a \
         pasted calculated field must be in it",
    );
    assert!(
        form.calc_order.contains(&outcome.field_id),
        "and it must be THIS field's reference",
    );
    assert!(
        field_named(&reloaded, "Live")
            .expect("field")
            .has_additional_actions,
        "the actions themselves travelled, JavaScript streams and all",
    );
}

// ---------------------------------------------------------------------------
// Multi-widget fields
// ---------------------------------------------------------------------------

/// A radio group travels as ONE unit, and lands as a rigid group.
#[test]
fn a_radio_group_carries_every_button_and_lands_as_a_group() {
    let mut s = session("forms/radio-group-form.pdf");
    let clip = s.copy_field("Priority").expect("copy");
    assert_eq!(clip.widget_count(), 3, "all three buttons travelled");
    assert_eq!(clip.button_kind(), Some(ButtonKind::Radio));

    let source_rects: Vec<Rect> = field_named(&s, "Priority")
        .expect("source")
        .widgets
        .iter()
        .filter_map(|w| w.rect)
        .collect();

    let outcome = s
        .paste_field(&clip, 0, rect(), &new_field("Priority2"))
        .expect("paste");
    assert_eq!(outcome.widget_ids.len(), 3);
    assert!(
        outcome
            .disclosures
            .iter()
            .any(|d| d.contains("MOVED as a unit")),
        "ignoring the drawn rectangle's SIZE is disclosed: {:?}",
        outcome.disclosures,
    );

    let reloaded = reload(&s);
    let pasted = field_named(&reloaded, "Priority2").expect("pasted group");
    assert_eq!(pasted.widgets.len(), 3);
    assert!(!pasted.merged, "three widgets means Shape B, never merged");

    // The GROUP's shape carries the meaning: each button keeps its own size
    // and its offset from the first.
    let pasted_rects: Vec<Rect> = pasted.widgets.iter().filter_map(|w| w.rect).collect();
    assert_eq!(pasted_rects.len(), 3);
    let first_source = source_rects.first().copied().expect("a source rect");
    let first_pasted = pasted_rects.first().copied().expect("a pasted rect");
    let (dx, dy) = (
        first_pasted.llx - first_source.llx,
        first_pasted.lly - first_source.lly,
    );
    assert!(
        (first_pasted.llx - rect().llx).abs() < 1e-9
            && (first_pasted.lly - rect().lly).abs() < 1e-9,
        "the group's first widget lands where the operator pointed",
    );
    for (had, got) in source_rects.iter().zip(pasted_rects.iter()) {
        assert!(
            (got.llx - had.llx - dx).abs() < 1e-9 && (got.lly - had.lly - dy).abs() < 1e-9,
            "every button moved by the SAME offset; {had:?} -> {got:?}",
        );
        assert!(
            (got.width() - had.width()).abs() < 1e-9 && (got.height() - had.height()).abs() < 1e-9,
            "and none of them was rescaled to fit a rectangle drawn by eye",
        );
    }

    // The export values are what make a radio group a radio group.
    let on_states: Vec<Vec<u8>> = pasted
        .widgets
        .iter()
        .flat_map(|w| w.on_states.clone())
        .collect();
    assert!(
        on_states.iter().any(|s| s == b"Green"),
        "the export values (/AP /N on-state names) travelled: {on_states:?}",
    );
}

/// Adding one more button to a group that already exports that value is
/// refused — two buttons that select together is a broken group.
#[test]
fn an_additional_radio_widget_with_a_taken_export_value_is_refused() {
    let mut s = session("forms/radio-group-form.pdf");
    let clip = s.copy_field("Priority").expect("copy");
    let err = s
        .paste_field(
            &clip,
            0,
            rect(),
            &FieldPastePolicy::AdditionalWidget {
                existing: "Priority".to_owned(),
            },
        )
        .expect_err("a duplicate export value must be refused");
    assert!(
        matches!(err, EditError::RadioExportValueTaken { .. }),
        "got {err:?}",
    );
}

/// A Shape B text field's several views also travel together.
#[test]
fn a_multi_widget_text_field_pastes_all_of_its_views() {
    let mut s = session("forms/multi-widget-form.pdf");
    let clip = s.copy_field("Reference").expect("copy");
    assert_eq!(clip.widget_count(), 3);
    s.paste_field(&clip, 0, rect(), &new_field("Reference2"))
        .expect("paste");
    let reloaded = reload(&s);
    let pasted = field_named(&reloaded, "Reference2").expect("field");
    assert_eq!(
        pasted.widgets.len(),
        3,
        "three views in, three views out -- one field, one value",
    );
}

// ---------------------------------------------------------------------------
// Refusals at the COPY, so the operator learns before placing
// ---------------------------------------------------------------------------

#[test]
fn copying_a_field_that_is_not_there_is_refused_by_name() {
    let s = session("forms/rich-field-form.pdf");
    let err = s.copy_field("NoSuchField").expect_err("refused");
    assert!(
        matches!(err, EditError::FieldNotFound { ref name } if name == "NoSuchField"),
        "got {err:?}",
    );
}

#[test]
fn copying_from_a_document_with_no_form_is_refused_by_name() {
    let s = session("hello.pdf");
    assert!(matches!(
        s.copy_field("Anything"),
        Err(EditError::NoInteractiveForm)
    ));
}

// ---------------------------------------------------------------------------
// Serialisation
// ---------------------------------------------------------------------------

/// A clip that has been through bytes pastes identically to one that has not.
///
/// This is the assertion that makes the cross-document gesture real: the
/// operator's two drawings are two files, and a clipboard that only worked
/// in-process would not cover the gesture he actually performs.
#[test]
fn a_clip_through_bytes_pastes_identically_to_one_that_stayed_in_memory() {
    let source = session("forms/rich-field-form.pdf");
    let clip = source.copy_field("TitleBlock.Revision").expect("copy");
    let wired = FieldClip::from_bytes(&clip.to_bytes()).expect("round trip");
    assert_eq!(wired, clip, "nothing is lost on the wire");

    let mut a = session("hello.pdf");
    let mut b = session("hello.pdf");
    a.paste_field(&clip, 0, rect(), &new_field("Direct"))
        .expect("paste in memory");
    b.paste_field(&wired, 0, rect(), &new_field("Direct"))
        .expect("paste from bytes");
    let (a_bytes, _) = a.to_full_bytes(&Default::default()).expect("save");
    let (b_bytes, _) = b.to_full_bytes(&Default::default()).expect("save");
    assert_eq!(
        a_bytes, b_bytes,
        "the two routes must produce the same document, byte for byte",
    );
}

// ---------------------------------------------------------------------------
// Accessibility (R105)
// ---------------------------------------------------------------------------

/// An undecided accessibility name is refused, exactly as it is at creation.
#[test]
fn an_undecided_accessibility_name_is_refused() {
    let mut s = session("forms/rich-field-form.pdf");
    let clip = s.copy_field("TitleBlock.Revision").expect("copy");
    let err = s
        .paste_field(
            &clip,
            0,
            rect(),
            &FieldPastePolicy::NewField {
                name: "Undecided".to_owned(),
                tooltip: PasteTooltip::Undecided,
                copy_value: false,
                copy_actions: false,
            },
        )
        .expect_err("R105");
    assert!(
        matches!(err, EditError::TooltipDecisionRequired { .. }),
        "got {err:?}",
    );
}

/// Reusing the source's `/TU` is a legitimate explicit answer — and it is
/// disclosed, because two fields announcing themselves identically to a
/// screen reader is invisible to a sighted operator.
#[test]
fn carrying_the_accessibility_name_is_allowed_and_disclosed() {
    let mut s = session("forms/rich-field-form.pdf");
    let clip = s.copy_field("TitleBlock.Revision").expect("copy");
    assert_eq!(clip.tooltip(), Some(b"Revision letter".as_slice()));
    let outcome = s
        .paste_field(
            &clip,
            0,
            rect(),
            &FieldPastePolicy::NewField {
                name: "Carried".to_owned(),
                tooltip: PasteTooltip::Carry,
                copy_value: false,
                copy_actions: false,
            },
        )
        .expect("paste");
    assert!(
        outcome
            .disclosures
            .iter()
            .any(|d| d.contains("accessibility name")),
        "{:?}",
        outcome.disclosures,
    );
    let reloaded = reload(&s);
    assert_eq!(
        field_named(&reloaded, "Carried")
            .expect("field")
            .alternate_name
            .as_deref(),
        Some(b"Revision letter".as_slice()),
    );
}

/// Declining writes no `/TU`, and the pasted field does not inherit one.
#[test]
fn declining_the_accessibility_name_does_not_leave_the_sources_behind() {
    let mut s = session("forms/rich-field-form.pdf");
    let clip = s.copy_field("TitleBlock.Revision").expect("copy");
    s.paste_field(&clip, 0, rect(), &new_field("Declined"))
        .expect("paste");
    let reloaded = reload(&s);
    assert_eq!(
        field_named(&reloaded, "Declined")
            .expect("field")
            .alternate_name,
        None,
        "an explicit declination must actually remove the carried /TU, not \
         merely fail to add one",
    );
}

// ---------------------------------------------------------------------------
// Undo
// ---------------------------------------------------------------------------

/// A paste is ONE undo entry, however many objects arrived.
#[test]
fn undoing_a_paste_removes_every_part_of_it() {
    let mut s = session("forms/radio-group-form.pdf");
    let clip = s.copy_field("Priority").expect("copy");
    s.paste_field(&clip, 0, rect(), &new_field("Priority2"))
        .expect("paste");
    assert!(field_named(&s, "Priority2").is_some());

    s.undo().expect("one undo entry");
    assert!(
        field_named(&s, "Priority2").is_none(),
        "the field, its three widgets, the /Annots patch and the /AcroForm \
         registration all undo together",
    );
    assert_eq!(
        forms::parse_acroform(&s.graph())
            .expect("form")
            .fields
            .len(),
        1,
        "back to the one field the file had",
    );
}

/// And undoing an additional-widget paste puts the field back in Shape A.
#[test]
fn undoing_an_additional_widget_paste_restores_the_merged_shape() {
    let mut s = session("forms/rich-field-form.pdf");
    let clip = s.copy_field("TitleBlock.Revision").expect("copy");
    s.paste_field(
        &clip,
        0,
        rect(),
        &FieldPastePolicy::AdditionalWidget {
            existing: "TitleBlock.Revision".to_owned(),
        },
    )
    .expect("paste");
    assert_eq!(
        field_named(&s, "TitleBlock.Revision")
            .expect("field")
            .widgets
            .len(),
        2,
    );
    s.undo().expect("one undo entry");
    let field = field_named(&s, "TitleBlock.Revision").expect("field");
    assert_eq!(field.widgets.len(), 1);
    assert!(
        field.merged,
        "the promotion to Shape B undoes with the widget that caused it",
    );
}

// ---------------------------------------------------------------------------
// Signature fields
// ---------------------------------------------------------------------------

/// A SIGNED signature field is refused at the copy, by name.
///
/// Its `/V` is a byte-range assertion about the document it was made in
/// (§12.7.4.5) and cannot travel. What COULD travel is the widget's baked
/// "signed by" artwork, into a file nobody signed — a plausible-looking void
/// artefact. pdfcer declines to make that object rather than making it and
/// disclosing it, which is the same posture redaction takes: some outputs are
/// not improved by a warning.
#[test]
fn a_signed_signature_field_is_refused_at_the_copy() {
    let s = session("signature/signed-full-coverage.pdf");
    let err = s.copy_field("Approval").expect_err("refused");
    assert!(
        matches!(err, EditError::SignedFieldNotCopyable { ref name } if name == "Approval"),
        "got {err:?}",
    );
}
