//! Image stamps (`EditSession::add_image_stamp`) and push-button icons
//! (`WidgetEdit::with_button_icon`, `/MK /I /TP /IF`, ISO 32000-1
//! §12.5.6.19 Table 189 and §12.7.7.3.2 Table 247). Synthetic documents and
//! images only; every assertion reads the saved-and-reloaded file.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::annot_author::CaptionPosition;
use pdfcer_core::document::Document;
use pdfcer_core::edit::{
    AppearanceOutcome, EditError, EditSession, ForeignAppearance, MarkupOptions, NewCheckBox,
    NewPushButton, ResizeOptions, WidgetEdit,
};
use pdfcer_core::forms::{self, MkColor};
use pdfcer_core::image_import::{self, ImportedImage};
use pdfcer_core::object::{Dict, ObjId, Object, Stream};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::writer::{SaveOptions, save_full};

const STAMP: Rect = Rect {
    llx: 20.0,
    lly: 20.0,
    urx: 68.0,
    ury: 44.0,
};
const BUTTON: Rect = Rect {
    llx: 40.0,
    lly: 80.0,
    urx: 160.0,
    ury: 104.0,
};

fn build(bodies: &[&str]) -> Document {
    let mut buf = b"%PDF-1.7\n".to_vec();
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
    Document::from_bytes(buf).expect("synthetic document parses")
}

fn blank() -> Document {
    build(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>",
    ])
}

/// A push button another producer drew: `/MK /I` names icon form 5 with
/// `/TP 2`, and `/AP /N` is form 6, which is not pdfcer's artwork.
fn foreign_button() -> Document {
    build(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R] >>",
        "<< /Type /Annot /Subtype /Widget /FT /Btn /Ff 65536 /T (B) /P 3 0 R \
         /Rect [20 20 100 50] /MK << /CA (Go) /I 5 0 R /TP 2 >> /AP << /N 6 0 R >> >>",
        "<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Length 18 >>\nstream\n\
         0 0 10 10 re f\n\nendstream",
        "<< /Type /XObject /Subtype /Form /BBox [0 0 80 30] /Length 24 >>\nstream\n\
         0 0 1 rg 0 0 80 30 re f\nendstream",
    ])
}

fn png(name: &str) -> ImportedImage {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/images")
        .join(name);
    image_import::import(&std::fs::read(path).expect("fixture")).expect("import")
}

fn saved(session: &EditSession) -> Vec<u8> {
    save_full(
        session.document(),
        &session.dirty_set(),
        &SaveOptions::identity(),
    )
    .expect("full rewrite")
    .0
}

fn reload(session: &EditSession) -> Document {
    Document::from_bytes(saved(session)).expect("reloads")
}

fn resolve(doc: &Document, o: &Object) -> Object {
    match o {
        Object::Reference(id) => doc.get(*id).expect("present").value.clone(),
        other => other.clone(),
    }
}

fn dict(doc: &Document, id: ObjId) -> Dict {
    match resolve(doc, &Object::Reference(id)) {
        Object::Dict(d) => d,
        Object::Stream(s) => s.dict,
        other => panic!("{id} is {other:?}"),
    }
}

fn sub(doc: &Document, d: &Dict, key: &[u8]) -> Dict {
    match d.get(key).map(|o| resolve(doc, o)) {
        Some(Object::Dict(d)) => d,
        other => panic!("{} is {other:?}", String::from_utf8_lossy(key)),
    }
}

fn reference(d: &Dict, key: &[u8]) -> ObjId {
    match d.get(key) {
        Some(Object::Reference(id)) => *id,
        other => panic!("{} is {other:?}", String::from_utf8_lossy(key)),
    }
}

fn stream(doc: &Document, id: ObjId) -> (Stream, String) {
    let Object::Stream(s) = doc.get(id).expect("present").value.clone() else {
        panic!("{id} is not a stream");
    };
    let raw = s.data_span.slice(doc.bytes()).expect("in range");
    let content = pdfcer_core::filters::decode_stream(&s.dict, raw).expect("decodes");
    (s, String::from_utf8_lossy(&content).into_owned())
}

fn name(n: &str) -> Object {
    Object::Name(n.as_bytes().into())
}

/// The one widget of the push button `field` in `doc`.
fn widget(doc: &Document, field: &str) -> forms::Widget {
    forms::parse_acroform(&doc.view())
        .expect("a form")
        .fields
        .into_iter()
        .find(|f| f.fully_qualified_name == field)
        .expect("field")
        .widgets
        .remove(0)
}

fn with_button(caption: &str) -> EditSession {
    let mut s = EditSession::new(blank());
    s.add_push_button(&NewPushButton::new(0, "Go", BUTTON, caption).declining_tooltip())
        .expect("push button");
    s
}

#[test]
fn an_image_stamp_draws_one_masked_image_fitted_in_its_rect() {
    let mut s = EditSession::new(blank());
    let image = png("rgba-half-clear.png");
    assert_eq!((image.width, image.height), (64, 32));
    let id = s
        .add_image_stamp(0, STAMP, &image, &MarkupOptions::default())
        .expect("stamp");
    let doc = reload(&s);
    let annot = dict(&doc, id);
    assert_eq!(annot.get(b"Subtype"), Some(&name("Stamp")));
    assert!(!annot.contains_key(b"Name"), "the face is the image");
    let ap = reference(&sub(&doc, &annot, b"AP"), b"N");
    let (form, content) = stream(&doc, ap);
    let xobjects = sub(&doc, &sub(&doc, &form.dict, b"Resources"), b"XObject");
    assert_eq!(xobjects.0.len(), 1, "one image XObject");
    let (img, _) = xobjects.0.first().expect("one");
    let img = dict(&doc, reference(&xobjects, &img.0));
    assert_eq!(img.get(b"Subtype"), Some(&name("Image")));
    assert!(img.contains_key(b"SMask"), "alpha became an /SMask");
    // 64x32 into 48x24 is the same aspect: the image fills the rect.
    assert!(content.contains("48 0 0 24 0 0 cm"), "{content}");
    let page = dict(&doc, ObjId::new(3, 0));
    assert_eq!(
        page.get(b"Annots"),
        Some(&Object::Array(vec![Object::Reference(id)]))
    );
}

#[test]
fn undoing_an_image_stamp_is_one_step_back_to_the_original_bytes() {
    let mut s = EditSession::new(blank());
    let before = saved(&s);
    s.add_image_stamp(
        0,
        STAMP,
        &png("rgba-half-clear.png"),
        &MarkupOptions::default(),
    )
    .expect("stamp");
    assert_eq!(s.undo_depth(), 1);
    s.undo().expect("undo");
    assert!(!s.can_undo());
    assert_eq!(saved(&s), before);
}

#[test]
fn resizing_an_image_stamp_refits_the_image() {
    let mut s = EditSession::new(blank());
    let id = s
        .add_image_stamp(
            0,
            STAMP,
            &png("rgba-half-clear.png"),
            &MarkupOptions::default(),
        )
        .expect("stamp");
    s.resize_annotation(id, (20.0, 20.0), 2.0, 2.0, &ResizeOptions::default())
        .expect("resize");
    let doc = reload(&s);
    let ap = reference(&sub(&doc, &dict(&doc, id), b"AP"), b"N");
    let (_, content) = stream(&doc, ap);
    assert!(content.contains("96 0 0 48 0 0 cm"), "{content}");
}

/// The icon form `/MK /I` names, and the image it draws.
fn icon_form_draws_an_image(doc: &Document, icon: ObjId) {
    let (form, content) = stream(doc, icon);
    assert_eq!(form.dict.get(b"Subtype"), Some(&name("Form")));
    assert!(content.contains("/Poster Do"), "{content}");
    let xobjects = sub(doc, &sub(doc, &form.dict, b"Resources"), b"XObject");
    let img = dict(doc, reference(&xobjects, b"Poster"));
    assert_eq!(img.get(b"Subtype"), Some(&name("Image")));
}

#[test]
fn a_push_button_icon_is_a_form_and_the_appearance_draws_it_alone() {
    let mut s = with_button("Go");
    let out = s
        .edit_widget(
            "Go",
            0,
            &WidgetEdit::new()
                .with_button_icon(&png("icon32.png"))
                .with_caption_position(CaptionPosition::IconOnly),
        )
        .expect("icon");
    assert_eq!(out.appearance, AppearanceOutcome::Regenerated);
    let doc = reload(&s);
    let w = widget(&doc, "Go");
    let wd = dict(&doc, w.id);
    let mk = sub(&doc, &wd, b"MK");
    let icon = reference(&mk, b"I");
    icon_form_draws_an_image(&doc, icon);
    assert_eq!(mk.get(b"TP"), Some(&Object::Integer(1)));
    let fit = sub(&doc, &mk, b"IF");
    assert_eq!(fit.get(b"SW"), Some(&name("A")));
    assert_eq!(fit.get(b"S"), Some(&name("P")));
    let (ap, content) = stream(&doc, reference(&sub(&doc, &wd, b"AP"), b"N"));
    assert!(content.contains("/Icon Do"), "{content}");
    assert!(!content.contains("BT"), "icon only: no caption: {content}");
    let res_icon = reference(
        &sub(&doc, &sub(&doc, &ap.dict, b"Resources"), b"XObject"),
        b"Icon",
    );
    assert_eq!(res_icon, icon);
    assert_eq!(w.icon, Some(icon));
    assert_eq!(w.caption_position, Some(CaptionPosition::IconOnly));
}

#[test]
fn undoing_a_button_icon_restores_mk_and_ap_exactly() {
    let mut s = with_button("Go");
    let before = saved(&s);
    s.edit_widget(
        "Go",
        0,
        &WidgetEdit::new().with_button_icon(&png("icon32.png")),
    )
    .expect("icon");
    assert_ne!(saved(&s), before);
    s.undo().expect("undo");
    assert_eq!(saved(&s), before);
}

#[test]
fn an_icon_without_a_position_goes_below_a_caption_or_alone_without_one() {
    for (caption, tp) in [("Go", 2), ("", 1)] {
        let mut s = with_button(caption);
        s.edit_widget(
            "Go",
            0,
            &WidgetEdit::new().with_button_icon(&png("icon32.png")),
        )
        .expect("icon");
        let doc = reload(&s);
        let mk = sub(&doc, &dict(&doc, widget(&doc, "Go").id), b"MK");
        assert_eq!(mk.get(b"TP"), Some(&Object::Integer(tp)), "{caption:?}");
    }
}

#[test]
fn a_position_edit_redraws_and_a_second_edit_still_owns_the_artwork() {
    let mut s = with_button("Go");
    s.edit_widget(
        "Go",
        0,
        &WidgetEdit::new().with_button_icon(&png("icon32.png")),
    )
    .expect("icon");
    let out = s
        .edit_widget(
            "Go",
            0,
            &WidgetEdit::new().with_caption_position(CaptionPosition::CaptionRight),
        )
        .expect("position");
    assert_eq!(out.appearance, AppearanceOutcome::Regenerated);
    let doc = reload(&s);
    let wd = dict(&doc, widget(&doc, "Go").id);
    let (_, content) = stream(&doc, reference(&sub(&doc, &wd, b"AP"), b"N"));
    assert!(
        content.contains("/Icon Do") && content.contains("BT"),
        "{content}"
    );
}

#[test]
fn clearing_the_icon_removes_i_and_tp_and_draws_the_caption() {
    let mut s = with_button("Go");
    s.edit_widget(
        "Go",
        0,
        &WidgetEdit::new().with_button_icon(&png("icon32.png")),
    )
    .expect("icon");
    let out = s
        .edit_widget("Go", 0, &WidgetEdit::new().without_button_icon())
        .expect("clear");
    assert_eq!(out.appearance, AppearanceOutcome::Regenerated);
    let doc = reload(&s);
    let wd = dict(&doc, widget(&doc, "Go").id);
    let mk = sub(&doc, &wd, b"MK");
    assert!(!mk.contains_key(b"I") && !mk.contains_key(b"TP"), "{mk:?}");
    let (_, content) = stream(&doc, reference(&sub(&doc, &wd, b"AP"), b"N"));
    assert!(
        !content.contains("/Icon Do") && content.contains("BT"),
        "{content}"
    );
}

#[test]
fn another_producers_icon_survives_an_edit_that_sets_none() {
    let mut s = EditSession::new(foreign_button());
    let out = s
        .edit_widget(
            "B",
            0,
            &WidgetEdit::new()
                .with_caption("Stop")
                .with_background(MkColor::Gray(0.5)),
        )
        .expect("edit");
    assert!(
        matches!(out.appearance, AppearanceOutcome::RecordedNotPainted(_)),
        "foreign artwork is kept: {:?}",
        out.appearance
    );
    let doc = reload(&s);
    let wd = dict(&doc, ObjId::new(4, 0));
    let mk = sub(&doc, &wd, b"MK");
    assert_eq!(mk.get(b"I"), Some(&Object::Reference(ObjId::new(5, 0))));
    assert_eq!(mk.get(b"TP"), Some(&Object::Integer(2)));
    assert_eq!(reference(&sub(&doc, &wd, b"AP"), b"N"), ObjId::new(6, 0));

    s.edit_widget(
        "B",
        0,
        &WidgetEdit::new().with_caption_position(CaptionPosition::CaptionAbove),
    )
    .expect("position");
    let doc = reload(&s);
    let mk = sub(&doc, &dict(&doc, ObjId::new(4, 0)), b"MK");
    assert_eq!(mk.get(b"I"), Some(&Object::Reference(ObjId::new(5, 0))));
    assert_eq!(mk.get(b"TP"), Some(&Object::Integer(3)));
}

/// Table 247 values may be indirect: `/SW 7 0 R` (`/N`) must stop the 10 pt
/// icon scaling up to the 80x30 button, as a direct `/N` would.
#[test]
fn an_indirect_icon_fit_value_is_honoured() {
    let doc = build(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R] >>",
        "<< /Type /Annot /Subtype /Widget /FT /Btn /Ff 65536 /T (B) /P 3 0 R \
         /Rect [20 20 100 50] /MK << /I 5 0 R /TP 1 /IF << /SW 7 0 R /A 8 0 R >> >> \
         /AP << /N 6 0 R >> >>",
        "<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Length 18 >>\nstream\n\
         0 0 10 10 re f\n\nendstream",
        "<< /Type /XObject /Subtype /Form /BBox [0 0 80 30] /Length 24 >>\nstream\n\
         0 0 1 rg 0 0 80 30 re f\nendstream",
        "/N",
        "[0 0]",
    ]);
    let mut s = EditSession::new(doc);
    s.edit_widget(
        "B",
        0,
        &WidgetEdit::new()
            .with_caption("Go")
            .with_foreign_appearance(ForeignAppearance::Replace),
    )
    .expect("edit");
    let doc = reload(&s);
    let n = reference(&sub(&doc, &dict(&doc, ObjId::new(4, 0)), b"AP"), b"N");
    let (_, content) = stream(&doc, n);
    let cm = content
        .lines()
        .find(|l| l.trim_end().ends_with(" cm"))
        .unwrap_or_else(|| panic!("no cm: {content}"));
    let m: Vec<&str> = cm.split_whitespace().take(4).collect();
    assert_eq!(m, ["1", "0", "0", "1"], "unscaled icon: {cm}");
}

#[test]
fn replacing_a_foreign_push_button_draws_its_own_icon() {
    let mut s = EditSession::new(foreign_button());
    let out = s
        .edit_widget(
            "B",
            0,
            &WidgetEdit::new()
                .with_caption("Stop")
                .with_foreign_appearance(ForeignAppearance::Replace),
        )
        .expect("edit");
    assert!(out.foreign_appearance_replaced);
    let doc = reload(&s);
    let wd = dict(&doc, ObjId::new(4, 0));
    let n = reference(&sub(&doc, &wd, b"AP"), b"N");
    assert_ne!(n, ObjId::new(6, 0));
    let (ap, content) = stream(&doc, n);
    assert!(
        content.contains("/Icon Do") && content.contains("Stop"),
        "{content}"
    );
    let icon = reference(
        &sub(&doc, &sub(&doc, &ap.dict, b"Resources"), b"XObject"),
        b"Icon",
    );
    assert_eq!(icon, ObjId::new(5, 0), "the producer's icon, kept");
}

#[test]
fn an_icon_on_a_check_box_is_refused_and_changes_nothing() {
    let mut s = EditSession::new(blank());
    s.add_check_box(&NewCheckBox::new(0, "Agree", BUTTON).declining_tooltip())
        .expect("check box");
    let before = saved(&s);
    let depth = s.undo_depth();
    for edit in [
        WidgetEdit::new().with_button_icon(&png("icon32.png")),
        WidgetEdit::new().with_caption_position(CaptionPosition::IconOnly),
    ] {
        let err = s.edit_widget("Agree", 0, &edit).expect_err("refused");
        assert!(matches!(err, EditError::NotAPushButton { .. }), "{err:?}");
    }
    assert_eq!(s.undo_depth(), depth);
    assert_eq!(saved(&s), before);
}

#[test]
fn a_default_icon_edit_replaces_a_foreign_push_button() {
    let mut s = EditSession::new(foreign_button());
    let out = s
        .edit_widget(
            "B",
            0,
            &WidgetEdit::new().with_button_icon(&png("icon32.png")),
        )
        .expect("icon");
    assert!(out.foreign_appearance_replaced);
    assert_eq!(out.appearance, AppearanceOutcome::Regenerated);
    let doc = reload(&s);
    let n = reference(&sub(&doc, &dict(&doc, ObjId::new(4, 0)), b"AP"), b"N");
    assert_ne!(n, ObjId::new(6, 0));
    let (_, content) = stream(&doc, n);
    assert!(content.contains("/Icon Do"), "{content}");
}

#[test]
fn a_default_caption_position_edit_replaces_a_foreign_push_button() {
    let mut s = EditSession::new(foreign_button());
    let out = s
        .edit_widget(
            "B",
            0,
            &WidgetEdit::new().with_caption_position(CaptionPosition::CaptionAbove),
        )
        .expect("position");
    assert!(out.foreign_appearance_replaced);
}

#[test]
fn keep_leaves_a_foreign_push_button_unpainted_on_an_icon_edit() {
    let mut s = EditSession::new(foreign_button());
    let out = s
        .edit_widget(
            "B",
            0,
            &WidgetEdit::new()
                .with_button_icon(&png("icon32.png"))
                .with_foreign_appearance(ForeignAppearance::Keep),
        )
        .expect("icon");
    assert!(!out.foreign_appearance_replaced);
    assert!(
        matches!(out.appearance, AppearanceOutcome::RecordedNotPainted(_)),
        "{:?}",
        out.appearance
    );
    let doc = reload(&s);
    let wd = dict(&doc, ObjId::new(4, 0));
    assert_eq!(reference(&sub(&doc, &wd, b"AP"), b"N"), ObjId::new(6, 0));
    assert_ne!(
        sub(&doc, &wd, b"MK").get(b"I"),
        Some(&Object::Reference(ObjId::new(5, 0))),
        "the new icon is still recorded"
    );
}
