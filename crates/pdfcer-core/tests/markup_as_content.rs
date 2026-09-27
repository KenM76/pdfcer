//! A Review markup shape drawn as ordinary page content
//! (`EditSession::add_markup_as_content`) rather than as an annotation.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::annot_author::{Color, MarkupSpec, Quad, TextMarkupKind};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, MarkupNote, MarkupOptions};
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::Object;
use pdfcer_core::page_tree::Rect;
use pdfcer_core::vector::VectorObject;
use pdfcer_core::writer::SaveOptions;

fn session() -> EditSession {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/synthetic/hello.pdf"
    ))
    .unwrap();
    EditSession::new(Document::from_bytes(bytes).unwrap())
}

fn square() -> MarkupSpec {
    MarkupSpec::Square {
        rect: Rect {
            llx: 100.0,
            lly: 100.0,
            urx: 200.0,
            ury: 150.0,
        },
        border: Some(Color::Rgb(1.0, 0.0, 0.0)),
        interior: None,
        border_width: 2.0,
        border_effect: None,
    }
}

fn object_count(s: &mut EditSession) -> usize {
    s.page_objects(0).unwrap().objects.len()
}

/// The page's `/Annots`, resolved; empty when absent.
fn annots(s: &EditSession) -> usize {
    let g = s.graph();
    let page = s.pages().unwrap()[0].id;
    g.resolved(page)
        .as_dict()
        .and_then(|d| d.get(b"Annots"))
        .map(|o| g.resolve(o))
        .and_then(Object::as_array)
        .map_or(0, <[Object]>::len)
}

/// The shape becomes the page's last objects, is a path, adds no
/// annotation, and survives a save and reopen as content.
#[test]
fn a_shape_drawn_as_content_is_a_page_object_not_an_annotation() {
    let mut s = session();
    let before = object_count(&mut s);
    let annots_before = annots(&s);
    let drawn = s
        .add_markup_as_content(0, &square(), &MarkupOptions::default())
        .unwrap();
    assert_eq!(drawn.objects.start, before);
    assert_eq!(drawn.objects.end, object_count(&mut s));
    assert!(!drawn.objects.is_empty());
    let objects = s.page_objects(0).unwrap();
    for i in drawn.objects.clone() {
        assert!(matches!(objects.objects[i], VectorObject::Path(_)));
    }
    assert_eq!(annots(&s), annots_before, "an annotation was authored");

    let saved = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let mut reopened = EditSession::new(Document::from_bytes(saved).unwrap());
    assert_eq!(object_count(&mut reopened), drawn.objects.end);
}

/// One undo removes the whole shape.
#[test]
fn one_undo_removes_the_shape() {
    let mut s = session();
    let before = object_count(&mut s);
    s.add_markup_as_content(0, &square(), &MarkupOptions::default())
        .unwrap();
    s.undo().unwrap();
    assert_eq!(object_count(&mut s), before);
}

/// The drawn shape is an ordinary object: the move verb takes it.
#[test]
fn the_drawn_shape_can_be_moved() {
    let mut s = session();
    let drawn = s
        .add_markup_as_content(0, &square(), &MarkupOptions::default())
        .unwrap();
    let i = drawn.objects.start;
    let x0 = s.page_objects(0).unwrap().objects[i].page_bbox().min.x;
    s.move_objects(0, &[i], 10.0, 0.0).unwrap();
    let x1 = s.page_objects(0).unwrap().objects[i].page_bbox().min.x;
    assert!((x1 - x0 - 10.0).abs() < 1e-6, "{x0} -> {x1}");
}

/// Page-content bytes appended by the call.
fn appended_content(s: &EditSession) -> String {
    let saved = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    String::from_utf8_lossy(&saved).into_owned()
}

/// Opacity, which page content cannot carry as `/CA` on an annotation,
/// becomes a bound `/ExtGState` with both alphas.
#[test]
fn opacity_is_drawn_through_an_ext_gstate() {
    let mut s = session();
    let drawn = s
        .add_markup_as_content(
            0,
            &square(),
            &MarkupOptions {
                opacity: Some(0.5),
                ..MarkupOptions::default()
            },
        )
        .unwrap();
    assert!(drawn.paste.resources_added >= 1);
    let text = appended_content(&s);
    assert!(text.contains("/CA 0.5"), "no stroking alpha");
    assert!(text.contains("/ca 0.5"), "no non-stroking alpha");
}

/// A highlight keeps its Multiply blend as page content.
#[test]
fn a_highlight_keeps_its_multiply_blend() {
    let mut s = session();
    let spec = MarkupSpec::TextMarkup {
        kind: TextMarkupKind::Highlight,
        quads: vec![Quad {
            ul: (100.0, 120.0),
            ur: (200.0, 120.0),
            ll: (100.0, 100.0),
            lr: (200.0, 100.0),
        }],
        color: Color::Rgb(1.0, 1.0, 0.0),
    };
    s.add_markup_as_content(0, &spec, &MarkupOptions::default())
        .unwrap();
    assert!(appended_content(&s).contains("/BM /Multiply"));
}

/// A note has nowhere to go in page content; it is disclosed, not dropped
/// silently.
#[test]
fn a_note_is_disclosed_as_not_written() {
    let mut s = session();
    let drawn = s
        .add_markup_as_content(
            0,
            &square(),
            &MarkupOptions {
                note: Some(MarkupNote::new("check this")),
                ..MarkupOptions::default()
            },
        )
        .unwrap();
    assert!(
        drawn.paste.disclosures.iter().any(|d| d.contains("note")),
        "{:?}",
        drawn.paste.disclosures
    );
}

/// An out-of-range opacity is refused before anything is written.
#[test]
fn a_bad_opacity_is_refused_and_writes_nothing() {
    let mut s = session();
    let before = object_count(&mut s);
    assert!(
        s.add_markup_as_content(
            0,
            &square(),
            &MarkupOptions {
                opacity: Some(4.0),
                ..MarkupOptions::default()
            },
        )
        .is_err()
    );
    assert_eq!(object_count(&mut s), before);
}
