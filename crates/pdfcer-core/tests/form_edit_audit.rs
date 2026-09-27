//! Field and widget edits must change only what they name, and whatever they
//! change must reach the file the way a reader will see it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{ChoiceOption, EditSession, FieldEdit, Visibility, WidgetEdit};
use pdfcer_core::forms;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{Dict, ObjId, Object};

fn assemble(bodies: &[String]) -> Vec<u8> {
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

fn session(bodies: &[&str]) -> EditSession {
    let owned: Vec<String> = bodies.iter().map(|s| (*s).to_owned()).collect();
    EditSession::new(Document::from_bytes(assemble(&owned)).expect("fixture parses"))
}

fn obj_dict(s: &EditSession, n: u32) -> Dict {
    let g = s.graph();
    match g.resolve(&Object::Reference(ObjId::new(n, 0))) {
        Object::Dict(d) => d.clone(),
        other => panic!("not a dictionary: {other:?}"),
    }
}

fn field(s: &EditSession, name: &str) -> forms::Field {
    forms::parse_acroform(&s.graph())
        .unwrap()
        .fields
        .into_iter()
        .find(|f| f.fully_qualified_name == name)
        .unwrap()
}

/// One text field, one widget (object 4), carrying `/F` = `flags`.
fn widget_with_flags(flags: i64) -> EditSession {
    let widget = format!(
        "<< /Type /Annot /Subtype /Widget /FT /Tx /T (Name) /P 3 0 R \
         /Rect [20 300 220 324] /F {flags} >>"
    );
    session(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Annots [4 0 R] >>",
        &widget,
    ])
}

// ---------------------------------------------------------------------------
// Visibility owns three `/F` bits, not the word
// ---------------------------------------------------------------------------

#[test]
fn setting_visibility_keeps_the_widgets_other_annotation_flags() {
    // Print | Locked | NoZoom | ReadOnly(annot).
    let mut s = widget_with_flags(4 | 128 | 8 | 64);
    s.edit_widget(
        "Name",
        0,
        &WidgetEdit::new().with_visibility(Visibility::PrintOnly),
    )
    .unwrap();
    assert_eq!(
        obj_dict(&s, 4).get(b"F").and_then(Object::as_int),
        Some(36 | 128 | 8 | 64),
        "only Hidden/Print/NoView change; Locked, NoZoom and ReadOnly survive"
    );
}

#[test]
fn visibility_reads_through_flags_it_does_not_own() {
    let s = widget_with_flags(4 | 8);
    assert_eq!(
        field(&s, "Name").widgets[0].visibility,
        Some(Visibility::VisibleAndPrints),
        "Print | NoZoom is visible-and-prints; NoZoom is not a visibility bit"
    );
    let s = widget_with_flags(4 | 2);
    assert_eq!(
        field(&s, "Name").widgets[0].visibility,
        None,
        "Print | Hidden is not one of the four and still reads as None"
    );
}

// ---------------------------------------------------------------------------
// Clearing an inheritable entry must not re-expose the parent's (12.7.3.1)
// ---------------------------------------------------------------------------

/// Parent `P` (object 4) carries `parent`; its kid `P.K` (object 5, a merged
/// text field + widget) carries `kid`.
fn parent_and_kid(parent: &str, kid: &str) -> EditSession {
    let parent = format!("<< /FT /Tx /T (P) /Kids [5 0 R] {parent} >>");
    let kid = format!(
        "<< /Type /Annot /Subtype /Widget /Parent 4 0 R /T (K) /P 3 0 R          /Rect [20 300 220 324] {kid} >>"
    );
    session(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Annots [5 0 R] >>",
        &parent,
        &kid,
    ])
}

#[test]
fn clearing_a_kids_last_flag_does_not_inherit_the_parents_flags() {
    // Parent is ReadOnly (1); the kid overrides with its own Required (2).
    let mut s = parent_and_kid("/Ff 1", "/Ff 2");
    s.edit_field("P.K", &FieldEdit::new().with_required(false))
        .unwrap();
    assert_eq!(
        obj_dict(&s, 5).get(b"Ff").and_then(Object::as_int),
        Some(0),
        "removing the kid's /Ff would make it inherit the parent's ReadOnly"
    );
    assert!(!field(&s, "P.K").flags.has(forms::FieldFlags::READ_ONLY));
}

#[test]
fn clearing_a_kids_last_flag_with_no_ancestor_flags_still_removes_the_key() {
    let mut s = parent_and_kid("", "/Ff 2");
    s.edit_field("P.K", &FieldEdit::new().with_required(false))
        .unwrap();
    assert!(obj_dict(&s, 5).get(b"Ff").is_none());
}

#[test]
fn clearing_a_kids_default_does_not_inherit_the_parents_default() {
    let mut s = parent_and_kid("/DV (parent)", "/DV (kid) /V (typed)");
    s.edit_field("P.K", &FieldEdit::new().clearing_default_value())
        .unwrap();
    s.reset_form(None).unwrap();
    let v = obj_dict(&s, 5).get(b"V").cloned();
    assert!(
        !matches!(&v, Some(Object::String(t)) if t.as_slice() == b"parent"),
        "a reset after clearing the default restored the parent's: {v:?}"
    );
}

// ---------------------------------------------------------------------------
// A choice field's redraw uses the options being written
// ---------------------------------------------------------------------------

fn normal_ap_bytes(s: &EditSession, widget: u32) -> Vec<u8> {
    let g = s.graph();
    let Some(Object::Dict(ap)) = obj_dict(s, widget).get(b"AP").map(|o| g.resolve(o).clone())
    else {
        return Vec::new();
    };
    let Some(Object::Stream(st)) = ap.get(b"N").map(|o| g.resolve(o).clone()) else {
        return Vec::new();
    };
    s.view().slice(st.data_span).unwrap_or_default().to_vec()
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn renaming_the_selected_options_label_redraws_the_new_label() {
    let mut s = session(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R]          /DA (/Helv 0 Tf 0 g) /DR << /Font << /Helv 5 0 R >> >> >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Annots [4 0 R] >>",
        "<< /Type /Annot /Subtype /Widget /FT /Ch /Ff 131072 /T (Fruit) /P 3 0 R          /Rect [20 300 220 324] /Opt [[(a) (Apple)] [(b) (Banana)]] /V (a)          /DA (/Helv 12 Tf 0 g) >>",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ]);
    s.edit_field(
        "Fruit",
        &FieldEdit::new().with_options(vec![
            ChoiceOption::new("a", "Avocado"),
            ChoiceOption::new("b", "Banana"),
        ]),
    )
    .unwrap();
    let ap = normal_ap_bytes(&s, 4);
    assert!(!ap.is_empty(), "the options change was redrawn");
    assert!(
        contains(&ap, b"Avocado") && !contains(&ap, b"Apple"),
        "the redraw shows the new label for export `a`: {}",
        String::from_utf8_lossy(&ap)
    );
}

// ---------------------------------------------------------------------------
// A property redraw uses the form's own font, as a fill does
// ---------------------------------------------------------------------------

/// A filled text field (object 4) under an `/AcroForm` whose `/DR` maps
/// `/TiRo` and `/F1` to Times; `field_da` is the field's own `/DA` entry.
fn times_form(form_da: &str, field_da: &str) -> EditSession {
    let catalog = format!(
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] {form_da}          /DR << /Font << /TiRo 5 0 R /F1 5 0 R >> >> >> >>"
    );
    let widget = format!(
        "<< /Type /Annot /Subtype /Widget /FT /Tx /T (Name) /P 3 0 R          /Rect [20 300 220 324] /V (Hello) {field_da} >>"
    );
    session(&[
        &catalog,
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Annots [4 0 R] >>",
        &widget,
        "<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman >>",
    ])
}

#[test]
fn a_property_redraw_uses_the_acroform_default_appearance() {
    let mut s = times_form("/DA (/TiRo 11 Tf 0 g)", "");
    s.edit_field("Name", &FieldEdit::new().with_multiline(true))
        .unwrap();
    let ap = normal_ap_bytes(&s, 4);
    assert!(
        contains(&ap, b"/TiRo 11 Tf") && !contains(&ap, b"/Helv"),
        "the inherited /DA face is kept: {}",
        String::from_utf8_lossy(&ap)
    );
}

#[test]
fn a_property_edit_on_a_field_naming_a_dr_font_does_not_fail() {
    let mut s = times_form("", "/DA (/F1 10 Tf 0 g)");
    s.edit_field("Name", &FieldEdit::new().with_multiline(true))
        .unwrap();
    let ap = normal_ap_bytes(&s, 4);
    assert!(
        contains(&ap, b"/F1 10 Tf"),
        "{}",
        String::from_utf8_lossy(&ap)
    );
}
