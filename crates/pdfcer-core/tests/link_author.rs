//! `add_link`, `set_link_target` and `set_link_border` (pdfcer-gui request
//! G161): a link is created, re-targeted and re-bordered, reads back through
//! `page_link_destinations`, and each edit is one undo entry.

use pdfcer_core::annot::{AnnotFlags, page_link_destinations};
use pdfcer_core::annot_author::{BorderDash, Color, MarkupSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{AnnotKind, CommandKind, EditError, EditSession, LinkBorder, LinkTarget};
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::outline::{DestView, Destination, DestinationReader};
use pdfcer_core::page_tree::{Rect, pages};
use pdfcer_core::writer::SaveOptions;

/// Two pages; page 0 carries a `/Link` whose `/A` is a JavaScript action;
/// the catalog has a legacy `/Dests` dictionary defining `old`.
const TWO: &[u8] = b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R /Dests << /old [4 0 R /Fit] >> >> endobj\n\
2 0 obj << /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >> endobj\n\
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Annots [5 0 R] >> endobj\n\
4 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >> endobj\n\
5 0 obj << /Type /Annot /Subtype /Link /Rect [10 10 60 30] /A << /S /JavaScript /JS (app.alert(1)) >> >> endobj\n\
trailer << /Size 6 /Root 1 0 R >>\n";

const OLD_LINK: ObjId = ObjId {
    num: 5,
    generation: 0,
};

fn session() -> EditSession {
    EditSession::new(Document::from_bytes(TWO.to_vec()).expect("rebuildable"))
}

fn reopened(s: &EditSession) -> Document {
    Document::from_bytes(s.to_full_bytes(&SaveOptions::identity()).expect("save").0)
        .expect("re-parse")
}

fn area() -> Rect {
    Rect {
        llx: 100.0,
        lly: 600.0,
        urx: 250.0,
        ury: 620.0,
    }
}

fn key(s: &EditSession, id: ObjId, k: &[u8]) -> Option<Object> {
    let Some(Object::Dict(d)) = s.value(id) else {
        panic!("not a dict")
    };
    d.get(k).cloned()
}

/// Destinations of page 0's links, after save and reopen.
fn page0_destinations(s: &EditSession) -> Vec<Destination> {
    let doc = reopened(s);
    let page = pages(&doc).expect("pages")[0].id;
    page_link_destinations(&doc, page, &DestinationReader::new(&doc))
        .links
        .into_iter()
        .map(|l| l.destination)
        .collect()
}

fn to_page_1() -> LinkTarget {
    LinkTarget::Page {
        page_index: 1,
        view: DestView::Fit,
    }
}

#[test]
fn a_page_link_without_border_reads_back_and_is_one_undo() {
    let mut s = session();
    let depth = s.undo_kinds().len();
    let id = s.add_link(0, area(), &to_page_1(), None).expect("link");
    assert_eq!(s.undo_kinds().len(), depth + 1);
    assert_eq!(
        key(&s, id, b"Border"),
        Some(Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(0)
        ])),
        "no border must be written: Table 164 defaults to a 1-point one"
    );
    assert!(key(&s, id, b"AP").is_none());
    let found = page0_destinations(&s);
    assert!(
        found.iter().any(|d| matches!(
            d,
            Destination::Page {
                page_index: 1,
                view: DestView::Fit
            }
        )),
        "{found:?}"
    );
    assert_eq!(
        s.undo(),
        Some(CommandKind::AddAnnotation {
            kind: AnnotKind::Link
        })
    );
    assert!(s.value(id).is_none());
}

#[test]
fn a_bordered_uri_link_gets_an_appearance_and_border_keys() {
    let mut s = session();
    let border = LinkBorder::new(2.0, Color::Rgb(0.0, 0.0, 1.0))
        .with_dash(BorderDash::new(vec![3.0, 2.0]).expect("dash"));
    let id = s
        .add_link(
            0,
            area(),
            &LinkTarget::Uri("https://example.com/a?b=1".into()),
            Some(&border),
        )
        .expect("link");
    assert!(key(&s, id, b"AP").is_some());
    assert!(key(&s, id, b"Border").is_none());
    let Some(Object::Dict(bs)) = key(&s, id, b"BS") else {
        panic!("no /BS")
    };
    assert!(bs.get(b"D").is_some(), "dash written: {bs:?}");
    assert!(key(&s, id, b"C").is_some());
    let Some(Object::Dict(a)) = key(&s, id, b"A") else {
        panic!("no /A")
    };
    assert_eq!(
        a.get(b"URI"),
        Some(&Object::String(b"https://example.com/a?b=1".to_vec()))
    );
    reopened(&s);
}

#[test]
fn a_named_target_is_written_in_its_own_namespace() {
    let mut s = session();
    // `old` lives in the legacy catalog `/Dests`: a name object.
    let id = s
        .add_link(0, area(), &LinkTarget::Named(b"old".to_vec()), None)
        .expect("legacy name");
    assert!(
        matches!(key(&s, id, b"Dest"), Some(Object::Name(n)) if n.as_bytes() == b"old"),
        "{:?}",
        key(&s, id, b"Dest")
    );
    assert!(
        page0_destinations(&s)
            .iter()
            .any(|d| matches!(d, Destination::Page { page_index: 1, .. })),
        "the legacy name resolves to page 1"
    );
    // A tree entry: a string.
    let mut s = session();
    s.add_named_destination(
        b"new",
        Destination::Page {
            page_index: 1,
            view: DestView::Fit,
        },
    )
    .expect("define");
    let id = s
        .add_link(0, area(), &LinkTarget::Named(b"new".to_vec()), None)
        .expect("tree name");
    assert_eq!(key(&s, id, b"Dest"), Some(Object::String(b"new".to_vec())));

    let err = s
        .add_link(0, area(), &LinkTarget::Named(b"nope".to_vec()), None)
        .unwrap_err();
    assert!(
        matches!(err, EditError::NamedDestinationNotFound { .. }),
        "{err:?}"
    );
}

#[test]
fn bad_input_is_refused_and_nothing_changes() {
    let mut s = session();
    let depth = s.undo_kinds().len();
    for (uri, why) in [
        ("", "empty"),
        ("https://e.com/caf\u{e9}", "ASCII"),
        ("a\nb", "control"),
    ] {
        let err = s
            .add_link(0, area(), &LinkTarget::Uri(uri.into()), None)
            .unwrap_err();
        assert!(
            matches!(&err, EditError::LinkUriInvalid { reason } if reason.contains(why)),
            "{uri:?}: {err:?}"
        );
    }
    let flat = Rect {
        llx: 10.0,
        lly: 10.0,
        urx: 10.0,
        ury: 50.0,
    };
    assert!(matches!(
        s.add_link(0, flat, &to_page_1(), None).unwrap_err(),
        EditError::EmptyGeometry
    ));
    let thin = LinkBorder::new(0.0, Color::Gray(0.0));
    assert!(matches!(
        s.add_link(0, area(), &to_page_1(), Some(&thin))
            .unwrap_err(),
        EditError::LinkBorderWidthInvalid { .. }
    ));
    assert!(matches!(
        s.add_link(
            0,
            area(),
            &LinkTarget::Page {
                page_index: 9,
                view: DestView::Fit
            },
            None
        )
        .unwrap_err(),
        EditError::PageOutOfRange { .. }
    ));
    assert_eq!(s.undo_kinds().len(), depth);
}

#[test]
fn retargeting_replaces_a_script_and_reports_it() {
    let mut s = session();
    let change = s.set_link_target(OLD_LINK, &to_page_1()).expect("retarget");
    assert_eq!(change.replaced_action.as_deref(), Some("JavaScript"));
    assert!(key(&s, OLD_LINK, b"A").is_none());
    assert!(matches!(key(&s, OLD_LINK, b"Dest"), Some(Object::Array(_))));
    assert_eq!(
        key(&s, OLD_LINK, b"Rect"),
        session().value(OLD_LINK).and_then(|o| match o {
            Object::Dict(d) => d.get(b"Rect").cloned(),
            _ => None,
        })
    );

    // A /Dest replaced by a URI: no action was replaced.
    let change = s
        .set_link_target(OLD_LINK, &LinkTarget::Uri("https://example.com".into()))
        .expect("to uri");
    assert_eq!(change.replaced_action, None);
    assert!(key(&s, OLD_LINK, b"Dest").is_none());

    assert_eq!(s.undo(), Some(CommandKind::SetLinkTarget));
    assert_eq!(s.undo(), Some(CommandKind::SetLinkTarget));
    assert!(matches!(key(&s, OLD_LINK, b"A"), Some(Object::Dict(_))));
}

#[test]
fn a_border_is_added_then_removed() {
    let mut s = session();
    let change = s
        .set_link_border(OLD_LINK, Some(&LinkBorder::new(1.5, Color::Gray(0.2))))
        .expect("border");
    assert!(!change.appearance_replaced);
    assert!(key(&s, OLD_LINK, b"AP").is_some());
    assert!(key(&s, OLD_LINK, b"A").is_some(), "target kept");

    let change = s.set_link_border(OLD_LINK, None).expect("none");
    assert!(change.appearance_replaced);
    assert!(key(&s, OLD_LINK, b"AP").is_none());
    assert!(key(&s, OLD_LINK, b"BS").is_none());
    assert!(key(&s, OLD_LINK, b"Border").is_some());
    assert_eq!(s.undo(), Some(CommandKind::SetLinkBorder));
    assert!(key(&s, OLD_LINK, b"AP").is_some());
}

#[test]
fn other_subtypes_and_locked_links_are_refused() {
    let mut s = session();
    let square = s
        .add_markup(
            0,
            &MarkupSpec::Square {
                rect: area(),
                border: Some(Color::Gray(0.0)),
                interior: None,
                border_width: 1.0,
                border_effect: None,
            },
        )
        .expect("square");
    assert!(matches!(
        s.set_link_target(square, &to_page_1()).unwrap_err(),
        EditError::LinkVerbOnOther { .. }
    ));
    assert!(matches!(
        s.set_link_border(square, None).unwrap_err(),
        EditError::LinkVerbOnOther { .. }
    ));
    s.set_annotation_flags(OLD_LINK, AnnotFlags(AnnotFlags::LOCKED))
        .expect("lock");
    assert!(matches!(
        s.set_link_target(OLD_LINK, &to_page_1()).unwrap_err(),
        EditError::AnnotationLocked { .. }
    ));
}
