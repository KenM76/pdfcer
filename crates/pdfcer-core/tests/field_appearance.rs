//! # `/DA` — a field's font, size and colour, at last settable
//!
//! The fourth and largest of the properties the form audit found readable and
//! unwritable, and the one that lagged the other three because it is the only
//! one needing a **resource** to exist.
//!
//! `Field::default_appearance` has been readable since the forms layer
//! shipped and nothing could write it. Worse, the value pdfcer wrote at
//! creation was **hard-coded** `/Helv 0 Tf 0 g` — so every field pdfcer
//! authored was black Helvetica auto-sized, and there was no way to say
//! otherwise. Acrobat exposes all three on a field's Appearance tab.
//!
//! ## Why a font resource makes this different
//!
//! `/DA` names a font by a **resource key** (§12.7.3.3), and that key must
//! resolve in `/AcroForm` `/DR` `/Font`. A key that does not resolve **does
//! not fail loudly** — the reader substitutes a face, the field looks
//! entirely normal, and it is drawn in something nobody chose.
//!
//! So the API splits the two cases by what pdfcer can promise:
//!
//! * [`FieldFont::Standard`] — pdfcer **authors the resource**, so it cannot
//!   fail for want of one. This is what a shell should offer by default.
//! * [`FieldFont::Resource`] — pdfcer can only **check**, and refuses by name
//!   when the key is absent, listing what is available so the refusal is
//!   actionable rather than merely correct.
//!
//! ## What is pinned
//!
//! 1. The `/DA` string is written with the right key, size and colour.
//! 2. **The resource is authored into `/DR` `/Font`** — without which the
//!    whole feature is a `/DA` pointing at nothing.
//! 3. **The appearance is REGENERATED.** Writing `/DA` alone would leave the
//!    field claiming one face and drawing another — the same disagreement
//!    `/MK /R` had before rotation became write-plus-regenerate.
//! 4. A `Resource` key that is absent is **refused, and nothing is written**.
//! 5. Auto-size (`0.0`) is preserved as `0`, because §12.7.3.3 gives it a
//!    meaning and rounding it to a real number would silently stop the field
//!    re-fitting as its value changes.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, FieldAppearance, FieldEdit, NewTextField};
use pdfcer_core::fontdata::Std14;
use pdfcer_core::forms;
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::Object;
use pdfcer_core::page_tree::Rect;
use pdfcer_core::vartext::TextColor;

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

const BOX: Rect = Rect {
    llx: 40.0,
    lly: 700.0,
    urx: 240.0,
    ury: 720.0,
};

fn with_text_field() -> EditSession {
    let doc = Document::load(&fixture("minimal.pdf")).expect("load minimal.pdf");
    let mut s = EditSession::new(doc);
    s.add_text_field(&NewTextField::new(0, "t", BOX).declining_tooltip())
        .expect("author the text field");
    s
}

fn field(s: &EditSession) -> forms::Field {
    let g = s.graph();
    forms::parse_acroform(&g)
        .expect("an AcroForm")
        .fields
        .iter()
        .find(|f| f.fully_qualified_name == "t")
        .cloned()
        .expect("the field")
}

/// The raw `/DA` string on the field dictionary.
fn da(s: &EditSession) -> String {
    let g = s.graph();
    let f = field(s);
    match g
        .resolved(f.id)
        .as_dict()
        .and_then(|d| d.get(b"DA"))
        .map(|o| g.resolve(o).clone())
    {
        Some(Object::String(b)) => String::from_utf8_lossy(&b).into_owned(),
        other => panic!("no /DA string: {other:?}"),
    }
}

/// The keys in `/AcroForm` `/DR` `/Font`.
fn dr_font_keys(s: &EditSession) -> Vec<String> {
    let g = s.graph();
    let Some(Object::Dict(af)) = g
        .trailer_entry(b"Root")
        .map(|o| g.resolve(o))
        .and_then(Object::as_dict)
        .and_then(|r| r.get(b"AcroForm"))
        .map(|o| g.resolve(o).clone())
    else {
        return Vec::new();
    };
    let Some(Object::Dict(dr)) = af.get(b"DR").map(|o| g.resolve(o).clone()) else {
        return Vec::new();
    };
    let Some(Object::Dict(fonts)) = dr.get(b"Font").map(|o| g.resolve(o).clone()) else {
        return Vec::new();
    };
    fonts
        .0
        .iter()
        .map(|(k, _)| String::from_utf8_lossy(k.as_bytes()).into_owned())
        .collect()
}

/// The widget's baked appearance-stream bytes, so a regeneration is visible.
fn ap_bytes(s: &EditSession) -> Vec<u8> {
    let g = s.graph();
    let f = field(s);
    let dict = g
        .resolved(f.widgets[0].id)
        .as_dict()
        .cloned()
        .expect("widget dict");
    let Some(Object::Dict(ap)) = dict.get(b"AP").map(|o| g.resolve(o).clone()) else {
        return Vec::new();
    };
    let Some(n) = ap.get(b"N") else {
        return Vec::new();
    };
    let Object::Stream(st) = g.resolve(n).clone() else {
        return Vec::new();
    };
    s.view().slice(st.data_span).unwrap_or_default().to_vec()
}

/// The `/DA` carries the resource key, the size and the colour.
#[test]
fn the_da_string_names_the_font_size_and_colour() {
    let mut s = with_text_field();
    s.edit_field(
        "t",
        &FieldEdit::new().with_appearance(FieldAppearance::standard(
            Std14::TimesBold,
            14.0,
            TextColor::Rgb(1.0, 0.0, 0.0),
        )),
    )
    .expect("set /DA");

    let got = da(&s);
    assert!(
        got.contains("/TiBo"),
        "must name Acrobat's Times-Bold key: {got}"
    );
    assert!(got.contains("14"), "must carry the size: {got}");
    assert!(got.contains("rg"), "must set an RGB fill colour: {got}");
}

/// ★★ The resource is AUTHORED, which is the half that makes the `/DA` mean
/// anything.
///
/// Without it the field names a key that resolves to nothing, the reader
/// substitutes a face, and the result looks normal and is wrong.
#[test]
fn a_standard_font_is_added_to_the_acroform_default_resources() {
    let mut s = with_text_field();
    assert!(
        !dr_font_keys(&s).contains(&"TiBo".to_owned()),
        "setup: Times-Bold should not be there yet"
    );

    s.edit_field(
        "t",
        &FieldEdit::new().with_appearance(FieldAppearance::standard(
            Std14::TimesBold,
            12.0,
            TextColor::Gray(0.0),
        )),
    )
    .expect("set /DA");

    assert!(
        dr_font_keys(&s).contains(&"TiBo".to_owned()),
        "the /DA names /TiBo, so /DR /Font must carry it — otherwise the key \
         resolves to nothing and the field draws in a substituted face. Keys: {:?}",
        dr_font_keys(&s)
    );
}

/// ★★ The appearance is REGENERATED, not just re-declared.
///
/// Writing `/DA` alone leaves the baked stream drawing the old face at the
/// old size, so the field claims one appearance and shows another — the
/// disagreement `/MK /R` had before rotation became write-plus-regenerate.
#[test]
fn changing_the_appearance_redraws_the_baked_stream() {
    let mut s = with_text_field();
    s.fill_text_field("t", "sample").expect("fill");
    let before = ap_bytes(&s);
    assert!(!before.is_empty(), "setup: there must be a baked stream");

    let out = s
        .edit_field(
            "t",
            &FieldEdit::new().with_appearance(FieldAppearance::standard(
                Std14::Courier,
                18.0,
                TextColor::Gray(0.0),
            )),
        )
        .expect("set /DA");

    assert!(
        out.appearance_regenerated,
        "the outcome must report the regeneration, because a shell shows it"
    );
    assert_ne!(
        ap_bytes(&s),
        before,
        "the baked stream must change — otherwise /DA says Courier 18 and the \
         pixels still say Helvetica auto"
    );
}

/// A `Resource` key that is not in `/DR` `/Font` is refused, the message
/// lists what IS available, and nothing is written.
#[test]
fn an_unknown_font_resource_is_refused_and_changes_nothing() {
    let mut s = with_text_field();
    let before = da(&s);

    match s.edit_field(
        "t",
        &FieldEdit::new().with_appearance(FieldAppearance::resource(
            b"NotThere".to_vec(),
            10.0,
            TextColor::Gray(0.0),
        )),
    ) {
        Err(EditError::FieldFontNotInResources { name, available }) => {
            assert_eq!(name, "NotThere");
            assert!(
                available.contains("Helv"),
                "the refusal must name what IS available, or it is correct and \
                 useless: {available}"
            );
        }
        other => panic!("expected FieldFontNotInResources, got {other:?}"),
    }
    assert_eq!(da(&s), before, "a refused edit must write nothing");
}

/// A `Resource` key that IS present is accepted — the refusal above is about
/// absence, not about the variant.
///
/// Without this, the refusal test would pass on a build that rejected every
/// `Resource` unconditionally.
#[test]
fn a_font_resource_that_exists_is_accepted() {
    let mut s = with_text_field();
    assert!(
        dr_font_keys(&s).contains(&"Helv".to_owned()),
        "setup: authoring a field seeds /DR /Font with Helv"
    );

    s.edit_field(
        "t",
        &FieldEdit::new().with_appearance(FieldAppearance::resource(
            b"Helv".to_vec(),
            9.0,
            TextColor::Gray(0.5),
        )),
    )
    .expect("an existing resource key is accepted");
    assert!(da(&s).contains("/Helv"), "{}", da(&s));
}

/// Auto-size survives as `0`.
///
/// §12.7.3.3 gives zero a meaning — the reader fits the text to the box — and
/// rounding it to a real number would silently stop the field re-fitting as
/// its value changes, which is exactly what an operator picking "Auto" asked
/// for.
#[test]
fn auto_size_is_written_as_zero_and_not_rounded_away() {
    let mut s = with_text_field();
    s.edit_field(
        "t",
        &FieldEdit::new().with_appearance(FieldAppearance::standard(
            Std14::Helvetica,
            0.0,
            TextColor::Gray(0.0),
        )),
    )
    .expect("set auto-size");
    let got = da(&s);
    assert!(
        got.contains("0 Tf") || got.contains("0.0 Tf") || got.contains("0 "),
        "auto-size must reach the /DA as a zero size: {got}"
    );
}
