//! Hand signatures: content added by `add_markup_as_content`, `add_text` and
//! `add_image` marked for a signature field, and found again by
//! `hand_signatures` (`pdfcer_core::hand_sig`, ISO 32000-1 §14.6).
//!
//! Every fixture is built inline from bytes this file authors (`LEGAL.md` §5).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::annot_author::{Color, MarkupSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, MarkupOptions, NewImage};
use pdfcer_core::hand_sig::{HandSignatureError, HandSignatureMark, hand_signatures};
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::page_tree::{Rect, pages};
use pdfcer_core::text_edit::{AddTextError, AddTextRequest};
use pdfcer_core::vector::{Matrix, TransformOptions};
use pdfcer_core::writer::SaveOptions;

const FIELD: &str = "Approver.Signature";

/// The `/Sig` field (object 6) and its widget (object 7) that the marks are
/// written for; nothing here may ever touch them.
const SIG_FIELD: ObjId = ObjId::new(6, 0);
const SIG_WIDGET: ObjId = ObjId::new(7, 0);

fn assemble(bodies: &[&str]) -> Vec<u8> {
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

/// One page whose content is `content`, a registered layer (object 4) and
/// an unsigned signature field (objects 6 and 7).
fn pdf(content: &str) -> Vec<u8> {
    let stream = format!(
        "<< /Length {} >>\nstream\n{content}\nendstream",
        content.len() + 1
    );
    assemble(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [6 0 R] /SigFlags 1 >> \
         /OCProperties << /OCGs [4 0 R] /D << /Order [4 0 R] >> >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 5 0 R \
         /Resources << >> /Annots [7 0 R] >>",
        "<< /Type /OCG /Name (Signatures) >>",
        &stream,
        "<< /FT /Sig /T (Approver) /Kids [7 0 R] >>",
        "<< /Type /Annot /Subtype /Widget /Parent 6 0 R /Rect [300 80 500 140] /P 3 0 R >>",
    ])
}

/// State-neutral, so an add appends without the overlay wrap.
const ONE_PATH: &str = "q 0 0 m 10 10 l S Q";

fn session(content: &str) -> EditSession {
    EditSession::new(Document::from_bytes(pdf(content)).expect("parses"))
}

fn ink() -> MarkupSpec {
    MarkupSpec::Ink {
        strokes: vec![vec![(320.0, 100.0), (360.0, 120.0), (400.0, 95.0)]],
        color: Color::Rgb(0.0, 0.0, 0.5),
        width: 1.5,
    }
}

fn signed(field: &str) -> MarkupOptions {
    MarkupOptions {
        hand_signature: Some(field.to_owned()),
        ..MarkupOptions::default()
    }
}

fn image() -> pdfcer_core::image_import::ImportedImage {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/images/rgb8.png");
    pdfcer_core::image_import::import(&std::fs::read(path).unwrap()).unwrap()
}

const IMAGE_RECT: Rect = Rect {
    llx: 300.0,
    lly: 80.0,
    urx: 500.0,
    ury: 140.0,
};

fn text() -> AddTextRequest {
    AddTextRequest::new(0, (310.0, 100.0), "J. Smith".to_owned())
}

/// The three verbs, each adding one hand signature for `field` to page 0.
#[derive(Debug, Clone, Copy)]
enum Verb {
    Markup,
    Text,
    Image,
}

const VERBS: [Verb; 3] = [Verb::Markup, Verb::Text, Verb::Image];

fn add(s: &mut EditSession, verb: Verb, field: &str) {
    match verb {
        Verb::Markup => {
            s.add_markup_as_content(0, &ink(), &signed(field)).unwrap();
        }
        Verb::Text => {
            s.add_text(&text().with_hand_signature(field)).unwrap();
        }
        Verb::Image => {
            let img = image();
            s.add_image(&NewImage::new(0, IMAGE_RECT, &img).as_hand_signature(field))
                .unwrap();
        }
    }
}

fn contains(outer: &Rect, inner: &Rect) -> bool {
    outer.llx <= inner.llx + 1e-6
        && outer.lly <= inner.lly + 1e-6
        && outer.urx >= inner.urx - 1e-6
        && outer.ury >= inner.ury - 1e-6
}

/// Where the verb's output paints, generously: the mark's bounds must lie
/// inside it and be non-degenerate.
fn painted_area(verb: Verb) -> Rect {
    match verb {
        Verb::Markup => Rect {
            llx: 300.0,
            lly: 80.0,
            urx: 420.0,
            ury: 140.0,
        },
        Verb::Text => Rect {
            llx: 300.0,
            lly: 90.0,
            urx: 420.0,
            ury: 120.0,
        },
        Verb::Image => IMAGE_RECT,
    }
}

fn assert_one_mark(found: &[HandSignatureMark], verb: Verb) {
    assert_eq!(found.len(), 1, "{verb:?}: {found:?}");
    assert_eq!(found[0].field, FIELD, "{verb:?}");
    let b = &found[0].bounds;
    assert!(b.urx > b.llx && b.ury > b.lly, "{verb:?}: degenerate {b:?}");
    assert!(contains(&painted_area(verb), b), "{verb:?}: {b:?}");
}

fn saved_streams(bytes: &[u8]) -> Vec<String> {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let view = doc.view();
    let page = &pages(&doc).unwrap()[0];
    page.contents
        .iter()
        .map(|id| {
            let Some(Object::Stream(st)) = view.graph().value(*id).cloned() else {
                panic!("{id:?} is not a stream");
            };
            String::from_utf8_lossy(view.slice(st.data_span).unwrap()).into_owned()
        })
        .collect()
}

/// Each verb's mark is found in the session, survives an incremental save
/// and reopen through both readers, is one undo entry, leaves the original
/// file a byte prefix of the save, and never touches the `/Sig` field.
#[test]
fn each_verb_round_trips_its_mark() {
    for verb in VERBS {
        let original = pdf(ONE_PATH);
        let mut s = session(ONE_PATH);
        add(&mut s, verb, FIELD);
        assert_eq!(s.undo_depth(), 1, "{verb:?}");
        assert_one_mark(&s.hand_signatures(0).unwrap(), verb);
        let dirty = s.dirty_set();
        assert!(
            !dirty.contains(SIG_FIELD) && !dirty.contains(SIG_WIDGET),
            "{verb:?}"
        );

        let (bytes, _) = s.to_incremental_bytes(&SaveOptions::identity()).unwrap();
        assert!(bytes.starts_with(&original), "{verb:?}: not minimal-diff");
        let doc = Document::from_bytes(bytes.clone()).unwrap();
        let page = &pages(&doc).unwrap()[0];
        assert_one_mark(&hand_signatures(&doc.view(), page).unwrap(), verb);
        let mut reopened = EditSession::new(doc);
        assert_one_mark(&reopened.hand_signatures(0).unwrap(), verb);
    }
}

/// Undo takes the mark with the content; nothing is left dirty.
#[test]
fn undo_removes_the_mark() {
    for verb in VERBS {
        let mut s = session(ONE_PATH);
        add(&mut s, verb, FIELD);
        s.undo().unwrap();
        assert!(s.hand_signatures(0).unwrap().is_empty(), "{verb:?}");
        assert!(s.dirty_set().is_empty(), "{verb:?}");
    }
}

/// Deleting what a mark encloses leaves an empty sequence, which is not a
/// signature; deleting something else leaves the mark reported.
#[test]
fn a_mark_whose_content_was_deleted_is_not_reported() {
    for verb in VERBS {
        let mut s = session(ONE_PATH);
        add(&mut s, verb, FIELD);
        let count = s.page_objects(0).unwrap().objects.len();
        // Object 0 is the page's own path; the verb's output follows it.
        s.delete_objects(0, &[0]).unwrap();
        assert_one_mark(&s.hand_signatures(0).unwrap(), verb);
        let added: Vec<usize> = (0..count - 1).collect();
        s.delete_objects(0, &added).unwrap();
        assert!(s.hand_signatures(0).unwrap().is_empty(), "{verb:?}");

        let (bytes, _) = s.to_incremental_bytes(&SaveOptions::identity()).unwrap();
        let doc = Document::from_bytes(bytes.clone()).unwrap();
        let page = &pages(&doc).unwrap()[0];
        assert!(hand_signatures(&doc.view(), page).unwrap().is_empty());
        assert!(
            saved_streams(&bytes).concat().contains("/pdfc_HandSig"),
            "{verb:?}: the empty sequence should remain in the bytes"
        );
    }
}

/// A mark names exactly the objects it encloses, in the session's and the
/// free reader's numbering alike, and transforming them moves the mark with
/// its bounds while the page's own path stays put.
#[test]
fn a_mark_names_its_objects_and_moves_with_them() {
    for verb in VERBS {
        let mut s = session(ONE_PATH);
        add(&mut s, verb, FIELD);
        let count = s.page_objects(0).unwrap().objects.len();
        let mark = s.hand_signatures(0).unwrap().remove(0);
        // Object 0 is the page's own path; the verb's output follows it.
        assert_eq!(mark.objects, (1..count).collect::<Vec<_>>(), "{verb:?}");
        let (bytes, _) = s.to_incremental_bytes(&SaveOptions::identity()).unwrap();
        let doc = Document::from_bytes(bytes).unwrap();
        let page = &pages(&doc).unwrap()[0];
        let read = hand_signatures(&doc.view(), page).unwrap();
        assert_eq!(read[0].objects, mark.objects, "{verb:?}");

        let own = s.page_objects(0).unwrap().objects[0].page_bbox();
        let m = Matrix::translate(20.0, 30.0);
        s.transform_objects(0, &mark.objects, m, TransformOptions::default())
            .unwrap();
        assert_eq!(s.page_objects(0).unwrap().objects[0].page_bbox(), own);
        let moved = s.hand_signatures(0).unwrap();
        assert_eq!(moved.len(), 1, "{verb:?}: {moved:?}");
        assert_eq!(moved[0].field, FIELD, "{verb:?}");
        let (a, b) = (&mark.bounds, &moved[0].bounds);
        for (got, want) in [
            (b.llx, a.llx + 20.0),
            (b.lly, a.lly + 30.0),
            (b.urx, a.urx + 20.0),
            (b.ury, a.ury + 30.0),
        ] {
            assert!((got - want).abs() < 1e-6, "{verb:?}: {a:?} -> {b:?}");
        }
    }
}

/// Two marks for two fields are reported in content order.
#[test]
fn marks_for_several_fields_are_told_apart() {
    let mut s = session(ONE_PATH);
    add(&mut s, Verb::Markup, "First");
    add(&mut s, Verb::Text, "Second");
    let fields: Vec<String> = s
        .hand_signatures(0)
        .unwrap()
        .into_iter()
        .map(|m| m.field)
        .collect();
    assert_eq!(fields, ["First", "Second"]);
}

/// An empty field name is refused before anything is written, on every
/// verb; the one-shot `add_text` refuses a hand signature outright.
#[test]
fn an_empty_field_name_is_refused() {
    let mut s = session(ONE_PATH);
    assert!(matches!(
        s.add_markup_as_content(0, &ink(), &signed("")),
        Err(EditError::HandSignature(HandSignatureError::EmptyFieldName))
    ));
    let err = s.add_text(&text().with_hand_signature("")).unwrap_err();
    assert!(matches!(
        err,
        AddTextError::HandSignature(ref inner)
            if matches!(**inner, EditError::HandSignature(HandSignatureError::EmptyFieldName))
    ));
    let img = image();
    assert!(matches!(
        s.add_image(&NewImage::new(0, IMAGE_RECT, &img).as_hand_signature("")),
        Err(EditError::HandSignature(HandSignatureError::EmptyFieldName))
    ));
    assert_eq!(s.undo_depth(), 0);
    assert!(s.dirty_set().is_empty());

    let doc = Document::from_bytes(pdf(ONE_PATH)).unwrap();
    assert!(matches!(
        pdfcer_core::text_edit::add_text(&doc, &text().with_hand_signature(FIELD)),
        Err(AddTextError::HandSignatureNeedsSession)
    ));
}

/// A hand signature is page content: the annotation routes refuse it.
#[test]
fn the_annotation_route_refuses_a_hand_signature() {
    let mut s = session(ONE_PATH);
    assert!(matches!(
        s.add_markup_with(0, &ink(), &signed(FIELD)),
        Err(EditError::HandSignature(HandSignatureError::NotPageContent))
    ));
    assert_eq!(s.undo_depth(), 0);
}

/// With a layer too, the layer's section is outermost and both survive a
/// save; the add is still one undo entry.
#[test]
fn a_layer_and_a_hand_signature_nest() {
    let layer = ObjId::new(4, 0);
    let mut s = session(ONE_PATH);
    let options = MarkupOptions {
        layer: Some(layer),
        ..signed(FIELD)
    };
    s.add_markup_as_content(0, &ink(), &options).unwrap();
    s.add_text(&text().on_layer(layer).with_hand_signature(FIELD))
        .unwrap();
    let img = image();
    s.add_image(
        &NewImage::new(0, IMAGE_RECT, &img)
            .on_layer(layer)
            .as_hand_signature(FIELD),
    )
    .unwrap();
    assert_eq!(s.undo_depth(), 3);
    let on_layer: Vec<Option<ObjId>> = s
        .page_objects(0)
        .unwrap()
        .objects
        .iter()
        .map(|o| o.oc())
        .collect();
    assert_eq!(on_layer.first(), Some(&None));
    assert!(
        on_layer[1..].iter().all(|oc| *oc == Some(layer)),
        "{on_layer:?}"
    );

    let (bytes, _) = s.to_incremental_bytes(&SaveOptions::identity()).unwrap();
    let streams = saved_streams(&bytes);
    for added in &streams[1..] {
        assert!(
            added.starts_with("/OC /OC1 BDC\n/pdfc_HandSig <<"),
            "{added:?}"
        );
        assert!(added.ends_with("\nEMC\nEMC\n"), "{added:?}");
    }
    let mut reopened = EditSession::new(Document::from_bytes(bytes).unwrap());
    assert_eq!(reopened.hand_signatures(0).unwrap().len(), 3);
}

/// A page that leaves a `cm` in effect is wrapped in a save/restore pair by
/// the add; that pair is not the verb's output and is not marked.
#[test]
fn the_overlay_wrap_is_not_marked() {
    for verb in VERBS {
        let mut s = session("1 0 0 1 5 5 cm 0 0 m 10 10 l S");
        add(&mut s, verb, FIELD);
        let (bytes, _) = s.to_incremental_bytes(&SaveOptions::identity()).unwrap();
        let streams = saved_streams(&bytes);
        assert_eq!(streams.len(), 4, "{verb:?}: expected a wrapped page");
        assert!(
            streams[0].starts_with("%pdfcer overlay wrap: save"),
            "{verb:?}"
        );
        assert!(
            streams[2].starts_with("%pdfcer overlay wrap: restore"),
            "{verb:?}"
        );
        assert!(streams[3].starts_with("/pdfc_HandSig"), "{verb:?}");
        let mut reopened = EditSession::new(Document::from_bytes(bytes).unwrap());
        assert_one_mark(&reopened.hand_signatures(0).unwrap(), verb);
    }
}

/// A page with no content has no marks, and an unknown page is refused.
#[test]
fn reading_an_empty_or_missing_page() {
    let mut s = EditSession::new(
        Document::from_bytes(assemble(&[
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>",
        ]))
        .unwrap(),
    );
    assert!(s.hand_signatures(0).unwrap().is_empty());
    assert!(matches!(
        s.hand_signatures(1),
        Err(EditError::PageOutOfRange { index: 1, count: 1 })
    ));
}
