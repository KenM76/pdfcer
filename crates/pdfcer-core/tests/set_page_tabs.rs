//! `set_page_tabs` writes a page's `/Tabs` and refuses the values the file
//! cannot carry: a PDF 2.0 name below 2.0, or a value its declared PDF/UA
//! part forbids (ISO 14289-1 §7.18.3: `S` only; ISO 14289-2 §8.9.3.3: `A`,
//! `W` or `S`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, PageTabs};
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::writer::SaveOptions;

/// One page, optionally with an annotation and an XMP packet declaring
/// `pdfuaid:part`. `tabs` is the page's starting `/Tabs` entry.
fn pdf(version: &str, ua_part: Option<&str>, annotated: bool, tabs: Option<&str>) -> Vec<u8> {
    let meta = if ua_part.is_some() {
        " /Metadata 5 0 R"
    } else {
        ""
    };
    let annots = if annotated { " /Annots [4 0 R]" } else { "" };
    let tabs = tabs.map_or(String::new(), |t| format!(" /Tabs /{t}"));
    let mut objects: Vec<Vec<u8>> = vec![
        format!("<< /Type /Catalog /Pages 2 0 R{meta} >>").into_bytes(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100]{annots}{tabs} >>")
            .into_bytes(),
        b"<< /Type /Annot /Subtype /Square /Rect [10 10 50 50] >>".to_vec(),
    ];
    if let Some(part) = ua_part {
        let xmp = format!(
            "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
             xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
             <rdf:Description rdf:about=\"\" \
             xmlns:pdfuaid=\"http://www.aiim.org/pdfua/ns/id/\" \
             pdfuaid:part=\"{part}\"/></rdf:RDF></x:xmpmeta>"
        );
        let mut s = format!(
            "<< /Type /Metadata /Subtype /XML /Length {} >>\nstream\n",
            xmp.len()
        )
        .into_bytes();
        s.extend_from_slice(xmp.as_bytes());
        s.extend_from_slice(b"\nendstream");
        objects.push(s);
    }
    let mut out = format!("%PDF-{version}\n").into_bytes();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(o);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let n = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {n}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

fn session(bytes: Vec<u8>) -> EditSession {
    EditSession::new(Document::from_bytes(bytes).expect("synthetic file loads"))
}

/// The page's `/Tabs` name as the session holds it, `None` when absent.
fn tabs_of(s: &EditSession) -> Option<String> {
    let Some(Object::Dict(page)) = s.value(ObjId::new(3, 0)) else {
        panic!("page 3 is a dictionary");
    };
    page.get(b"Tabs")
        .and_then(Object::as_name)
        .map(|n| String::from_utf8_lossy(n.as_bytes()).into_owned())
}

#[test]
fn structure_order_is_written_and_survives_a_save() {
    let mut s = session(pdf("1.7", None, true, None));
    let before = s.set_page_tabs(0, PageTabs::Structure).expect("S is legal");
    assert_eq!(before, PageTabs::Absent);
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::default())
        .expect("save");
    let reopened = session(bytes);
    assert_eq!(tabs_of(&reopened).as_deref(), Some("S"));
}

#[test]
fn a_pdf20_value_is_refused_below_20_and_allowed_at_20() {
    let mut old = session(pdf("1.7", None, true, None));
    let err = old.set_page_tabs(0, PageTabs::ArrayOrder).unwrap_err();
    assert!(
        matches!(err, EditError::TabsNeedPdf20 { value: "A", .. }),
        "{err:?}"
    );
    assert!(!old.can_undo(), "a refusal records nothing");

    let mut new = session(pdf("2.0", None, true, None));
    new.set_page_tabs(0, PageTabs::WidgetOrder)
        .expect("W is a 2.0 value");
    assert_eq!(tabs_of(&new).as_deref(), Some("W"));
}

#[test]
fn pdfua1_permits_only_structure_order() {
    let mut s = session(pdf("2.0", Some("1"), true, Some("S")));
    for tabs in [PageTabs::Row, PageTabs::ArrayOrder] {
        let err = s.set_page_tabs(0, tabs).unwrap_err();
        assert!(matches!(err, EditError::TabsBreakPdfUa { .. }), "{err:?}");
    }
    assert_eq!(tabs_of(&s).as_deref(), Some("S"));
}

#[test]
fn pdfua2_permits_array_order_but_not_column() {
    let mut s = session(pdf("2.0", Some("2"), true, Some("S")));
    s.set_page_tabs(0, PageTabs::ArrayOrder)
        .expect("UA-2 permits A");
    let err = s.set_page_tabs(0, PageTabs::Column).unwrap_err();
    assert!(
        matches!(&err, EditError::TabsBreakPdfUa { part, .. } if part == "2"),
        "{err:?}"
    );
    assert_eq!(tabs_of(&s).as_deref(), Some("A"));
}

#[test]
fn removal_breaks_pdfua_only_where_there_are_annotations() {
    let mut annotated = session(pdf("1.7", Some("1"), true, Some("S")));
    let err = annotated.set_page_tabs(0, PageTabs::Absent).unwrap_err();
    assert!(matches!(err, EditError::TabsBreakPdfUa { .. }), "{err:?}");

    let mut bare = session(pdf("1.7", Some("1"), false, Some("S")));
    bare.set_page_tabs(0, PageTabs::Absent)
        .expect("no annotations, no rule");
    assert_eq!(tabs_of(&bare), None);
}

#[test]
fn an_undefined_name_is_refused() {
    let mut s = session(pdf("2.0", None, true, None));
    let err = s
        .set_page_tabs(0, PageTabs::Other("Q".to_owned()))
        .unwrap_err();
    assert!(
        matches!(err, EditError::TabsValueUndefined { .. }),
        "{err:?}"
    );
}

#[test]
fn an_unchanged_value_records_nothing_and_undo_restores() {
    let mut s = session(pdf("1.7", None, true, Some("R")));
    assert_eq!(s.set_page_tabs(0, PageTabs::Row).unwrap(), PageTabs::Row);
    assert!(!s.can_undo(), "writing the value already there is no edit");

    assert_eq!(s.set_page_tabs(0, PageTabs::Column).unwrap(), PageTabs::Row);
    assert_eq!(tabs_of(&s).as_deref(), Some("C"));
    s.undo().expect("one command to undo");
    assert_eq!(tabs_of(&s).as_deref(), Some("R"));
}
