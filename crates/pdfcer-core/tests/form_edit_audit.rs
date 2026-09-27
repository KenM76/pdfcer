//! Field and widget edits must change only what they name, and whatever they
//! change must reach the file the way a reader will see it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{
    BorderSpec, BorderStyle, ChoiceOption, EditSession, FieldEdit, NewCheckBox, Visibility,
    WidgetEdit,
};
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

// ---------------------------------------------------------------------------
// A per-widget edit leaves the field's other widgets alone
// ---------------------------------------------------------------------------

#[test]
fn a_widget_edit_does_not_redraw_its_siblings() {
    // One text field, two widgets (5, 6), each with its own appearance (7, 8).
    let mut s = session(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R]          /DA (/Helv 0 Tf 0 g) >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Annots [5 0 R 6 0 R] >>",
        "<< /FT /Tx /T (Name) /V (Hi) /Kids [5 0 R 6 0 R] >>",
        "<< /Type /Annot /Subtype /Widget /Parent 4 0 R /P 3 0 R          /Rect [20 300 220 324] /AP << /N 7 0 R >> >>",
        "<< /Type /Annot /Subtype /Widget /Parent 4 0 R /P 3 0 R          /Rect [20 200 220 224] /AP << /N 8 0 R >> >>",
        "<< /Type /XObject /Subtype /Form /BBox [0 0 200 24] /Length 0 >>
stream

endstream",
        "<< /Type /XObject /Subtype /Form /BBox [0 0 200 24] /Length 0 >>
stream

endstream",
    ]);
    let before = obj_dict(&s, 6);
    let out = s
        .edit_widget(
            "Name",
            0,
            &WidgetEdit::new().with_background(forms::MkColor::Gray(0.5)),
        )
        .unwrap();
    assert_eq!(out.siblings_untouched, 1);
    assert_eq!(
        obj_dict(&s, 6),
        before,
        "the sibling widget's dictionary (and its /AP) was rewritten"
    );
}

/// A text field with a stated border colour and, optionally, a `/BS`.
fn bordered_text_field(bs: &str) -> EditSession {
    session(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] /DA (/Helv 0 Tf 0 g) >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Annots [4 0 R] >>",
        &format!(
            "<< /FT /Tx /T (Name) /V (Hi) /Type /Annot /Subtype /Widget /P 3 0 R \
             /Rect [20 300 220 324] /MK << /BC [0] >> {bs} >>"
        ),
    ])
}

/// The bytes of one `/AP` `/N` state stream of a button widget.
fn state_ap_bytes(s: &EditSession, widget: u32, state: &[u8]) -> Vec<u8> {
    let g = s.graph();
    let Some(Object::Dict(ap)) = obj_dict(s, widget).get(b"AP").map(|o| g.resolve(o).clone())
    else {
        return Vec::new();
    };
    let Some(Object::Dict(n)) = ap.get(b"N").map(|o| g.resolve(o).clone()) else {
        return Vec::new();
    };
    let Some(Object::Stream(st)) = n.get(state).map(|o| g.resolve(o).clone()) else {
        return Vec::new();
    };
    s.view().slice(st.data_span).unwrap_or_default().to_vec()
}

#[test]
fn a_border_edit_draws_the_width_and_the_dash() {
    let mut s = bordered_text_field("");
    s.edit_widget(
        "Name",
        0,
        &WidgetEdit::new().with_border(BorderSpec {
            style: BorderStyle::Dashed,
            width: 3.0,
        }),
    )
    .unwrap();
    let ap = normal_ap_bytes(&s, 4);
    assert!(contains(&ap, b"3 w"), "{}", String::from_utf8_lossy(&ap));
    assert!(
        contains(&ap, b"[3] 0 d"),
        "{}",
        String::from_utf8_lossy(&ap)
    );
    assert!(
        contains(&ap, b"1.5 1.5 197 21 re"),
        "{}",
        String::from_utf8_lossy(&ap)
    );
}

#[test]
fn a_resize_redraws_in_the_widgets_existing_border_style() {
    let mut s = bordered_text_field("/BS << /S /U /W 2 >>");
    s.edit_widget(
        "Name",
        0,
        &WidgetEdit::new().with_rect(pdfcer_core::page_tree::Rect {
            llx: 20.0,
            lly: 300.0,
            urx: 120.0,
            ury: 330.0,
        }),
    )
    .unwrap();
    let ap = normal_ap_bytes(&s, 4);
    assert!(contains(&ap, b"2 w"), "{}", String::from_utf8_lossy(&ap));
    // Underline: one bottom edge, no rectangle.
    assert!(contains(&ap, b"0 1 m"), "{}", String::from_utf8_lossy(&ap));
    assert!(
        !contains(&ap, b" re\nS"),
        "{}",
        String::from_utf8_lossy(&ap)
    );
}

#[test]
fn a_zero_width_border_draws_no_frame_on_a_check_box() {
    let mut s = session(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] >>",
    ]);
    let rect = pdfcer_core::page_tree::Rect {
        llx: 20.0,
        lly: 300.0,
        urx: 40.0,
        ury: 320.0,
    };
    s.add_check_box(&NewCheckBox::new(0, "Ok", rect).declining_tooltip())
        .unwrap();
    let widget = field(&s, "Ok").widgets[0].id.num;
    assert!(contains(&state_ap_bytes(&s, widget, b"Off"), b"1 w"));
    let out = s
        .edit_widget(
            "Ok",
            0,
            &WidgetEdit::new().with_border(BorderSpec {
                style: BorderStyle::Solid,
                width: 0.0,
            }),
        )
        .unwrap();
    assert!(out.appearance_regenerated);
    // No background and no frame: the /Off state is now an empty stream.
    let off = state_ap_bytes(&s, widget, b"Off");
    assert!(
        !contains(
            &off, b" w
"
        ),
        "{}",
        String::from_utf8_lossy(&off)
    );
}

#[test]
fn a_beveled_border_draws_its_light_and_shadow_bands() {
    let mut s = bordered_text_field("");
    s.edit_widget(
        "Name",
        0,
        &WidgetEdit::new().with_border(BorderSpec {
            style: BorderStyle::Beveled,
            width: 2.0,
        }),
    )
    .unwrap();
    let ap = normal_ap_bytes(&s, 4);
    assert!(contains(&ap, b"1 g"), "{}", String::from_utf8_lossy(&ap));
    assert!(contains(&ap, b"0.5 g"), "{}", String::from_utf8_lossy(&ap));
}

#[test]
fn filling_a_field_redraws_it_in_its_border_style() {
    let mut s = bordered_text_field("/BS << /S /D /W 2 >>");
    s.fill_text_field("Name", "Typed").unwrap();
    let ap = normal_ap_bytes(&s, 4);
    assert!(contains(&ap, b"2 w"), "{}", String::from_utf8_lossy(&ap));
    assert!(
        contains(&ap, b"[3] 0 d"),
        "{}",
        String::from_utf8_lossy(&ap)
    );
}

#[test]
fn re_setting_a_border_keeps_and_draws_the_widgets_dash_pattern() {
    let mut s = bordered_text_field("/BS << /S /D /D [4 2] /W 1 >>");
    s.edit_widget(
        "Name",
        0,
        &WidgetEdit::new().with_border(BorderSpec {
            style: BorderStyle::Dashed,
            width: 2.0,
        }),
    )
    .unwrap();
    let g = s.graph();
    let Some(Object::Dict(bs)) = obj_dict(&s, 4).get(b"BS").map(|o| g.resolve(o).clone()) else {
        panic!("no /BS");
    };
    assert!(bs.get(b"D").is_some(), "the /D dash pattern was dropped");
    let ap = normal_ap_bytes(&s, 4);
    assert!(contains(&ap, b"2 w"), "{}", String::from_utf8_lossy(&ap));
    assert!(
        contains(&ap, b"[4 2] 0 d"),
        "{}",
        String::from_utf8_lossy(&ap)
    );
}

#[test]
fn a_fill_draws_the_widgets_own_dash_pattern() {
    let mut s = bordered_text_field("/BS << /S /D /D [5 1] >>");
    s.fill_text_field("Name", "Typed").unwrap();
    let ap = normal_ap_bytes(&s, 4);
    assert!(
        contains(&ap, b"[5 1] 0 d"),
        "{}",
        String::from_utf8_lossy(&ap)
    );
}

#[test]
fn a_dash_array_with_no_style_reads_as_dashed() {
    let s = bordered_text_field("/BS << /D [4 2] >>");
    let border = field(&s, "Name").widgets[0].border.expect("a /BS");
    assert_eq!(border.style, BorderStyle::Dashed);
}

#[test]
fn a_background_colour_is_stored_with_the_digits_it_was_given() {
    let mut s = bordered_text_field("");
    s.edit_widget(
        "Name",
        0,
        &WidgetEdit::new().with_background(forms::MkColor::Rgb(0.2, 0.4, 0.6)),
    )
    .unwrap();
    let g = s.graph();
    let Some(Object::Dict(mk)) = obj_dict(&s, 4).get(b"MK").map(|o| g.resolve(o).clone()) else {
        panic!("no /MK");
    };
    assert_eq!(
        mk.get(b"BG"),
        Some(&Object::Array(vec![
            Object::Real(0.2),
            Object::Real(0.4),
            Object::Real(0.6)
        ])),
        "f32 widening wrote a precision the operator never chose"
    );
}
