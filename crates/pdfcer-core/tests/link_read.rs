//! `EditSession::link` (pdfcer-gui request G178): a link reads back as the
//! `LinkTarget` and `LinkBorder` the link verbs take, with a named target
//! kept as its name and the border read as the file states it.

use pdfcer_core::annot_author::{BorderDash, Color, ColorEntry};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, LinkBorder, LinkTarget};
use pdfcer_core::object::ObjId;
use pdfcer_core::outline::DestView;
use pdfcer_core::page_tree::Rect;

/// Two pages; page 0 carries four foreign links and a square. The catalog's
/// legacy `/Dests` defines `old`.
const DOC: &[u8] = b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R /Dests << /old [4 0 R /Fit] >> >> endobj\n\
2 0 obj << /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >> endobj\n\
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Annots [5 0 R 6 0 R 7 0 R 8 0 R 9 0 R] >> endobj\n\
4 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >> endobj\n\
5 0 obj << /Type /Annot /Subtype /Link /Rect [10 10 60 30] /A << /S /JavaScript /JS (app.alert(1)) >> >> endobj\n\
6 0 obj << /Type /Annot /Subtype /Link /Rect [10 40 60 60] /Dest /old /Border [0 0 2 [4 2]] /C [1 0 0] >> endobj\n\
7 0 obj << /Type /Annot /Subtype /Link /Rect [10 70 60 90] /A << /S /URI /URI (https://example.test/a) >> /BS << /W 3 /S /D >> /C [] >> endobj\n\
8 0 obj << /Type /Annot /Subtype /Square /Rect [10 100 60 120] >> endobj\n\
9 0 obj << /Type /Annot /Subtype /Link /Rect [10 130 60 150] /Dest [4 0 R /FitH 500] /C (red) >> endobj\n\
trailer << /Size 10 /Root 1 0 R >>\n";

fn id(num: u32) -> ObjId {
    ObjId { num, generation: 0 }
}

fn session() -> EditSession {
    EditSession::new(Document::from_bytes(DOC.to_vec()).expect("rebuildable"))
}

fn area() -> Rect {
    Rect {
        llx: 100.0,
        lly: 600.0,
        urx: 250.0,
        ury: 620.0,
    }
}

#[test]
fn what_the_link_verbs_write_reads_back_unchanged() {
    let mut s = session();
    let page = LinkTarget::Page {
        page_index: 1,
        view: DestView::Fit,
    };
    let link = s.add_link(0, area(), &page, None).expect("link");
    let read = s.link(link).expect("read");
    assert_eq!(read.target, Some(page));
    assert_eq!((read.border, read.action), (None, None));

    let border = LinkBorder::new(2.0, Color::Rgb(0.0, 0.0, 1.0))
        .with_dash(BorderDash::new(vec![3.0, 1.0]).expect("dash"));
    s.set_link_border(link, Some(&border)).expect("border");
    let read = s.link(link).expect("read");
    assert_eq!(read.border, Some(border));
    assert_eq!(
        read.border_color,
        ColorEntry::Device(Color::Rgb(0.0, 0.0, 1.0))
    );

    for target in [
        LinkTarget::Named(b"old".to_vec()),
        LinkTarget::Uri("https://example.test/b".to_owned()),
    ] {
        s.set_link_target(link, &target).expect("retarget");
        assert_eq!(s.link(link).expect("read").target, Some(target));
    }
}

#[test]
fn a_named_destination_stays_a_name_and_border_array_is_read() {
    let read = session().link(id(6)).expect("read");
    assert_eq!(read.target, Some(LinkTarget::Named(b"old".to_vec())));
    assert_eq!(
        read.border,
        Some(
            LinkBorder::new(2.0, Color::Rgb(1.0, 0.0, 0.0))
                .with_dash(BorderDash::new(vec![4.0, 2.0]).expect("dash"))
        )
    );
}

#[test]
fn an_empty_colour_draws_no_border_and_a_uri_keeps_its_address() {
    let read = session().link(id(7)).expect("read");
    assert_eq!(
        read.target,
        Some(LinkTarget::Uri("https://example.test/a".to_owned()))
    );
    assert_eq!(read.border, None);
    assert_eq!(read.border_color, ColorEntry::Empty);
}

#[test]
fn another_action_is_named_and_an_unstated_border_is_the_default() {
    let read = session().link(id(5)).expect("read");
    assert_eq!(read.target, None);
    assert_eq!(read.action.as_deref(), Some("JavaScript"));
    assert_eq!(read.border, Some(LinkBorder::new(1.0, Color::Gray(0.0))));
    assert_eq!(read.border_color, ColorEntry::Absent);
}

#[test]
fn an_explicit_destination_reads_as_a_page_and_a_bad_colour_is_stated() {
    let read = session().link(id(9)).expect("read");
    assert!(matches!(
        read.target,
        Some(LinkTarget::Page { page_index: 1, .. })
    ));
    assert_eq!(read.border_color, ColorEntry::Malformed);
    assert_eq!(read.border, Some(LinkBorder::new(1.0, Color::Gray(0.0))));
}

#[test]
fn a_non_link_is_refused() {
    assert!(matches!(
        session().link(id(8)),
        Err(EditError::LinkVerbOnOther { .. })
    ));
    assert!(matches!(
        session().link(id(77)),
        Err(EditError::AnnotationNotFound { .. })
    ));
}
