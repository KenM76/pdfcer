#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test code: a failed lookup should fail the test loudly

use super::tests_support::build_pdf;
use super::*;
use crate::document::Document;
use crate::graph::ObjectGraph;
use crate::object::{Dict, Object};

/// Three pages (3, 4, 5). Element 6 marks content on page 4 by MCID;
/// element 7 has no `/Pg` and reaches page 3 through a marked-content
/// reference; element 9 has no kids. Link 12 (`/Dest` naming 6), link 13
/// (GoTo with only an `/SD` naming 7) and link 15 (`/Dest` naming 9) sit on
/// page 3; outline item 14 names 6.
fn tagged() -> Document {
    build_pdf(&[
        (
            1,
            "<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 10 0 R /Outlines 11 0 R >>",
        ),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 /MediaBox [0 0 200 200] >>",
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Annots [12 0 R 13 0 R 15 0 R] >>",
        ),
        (4, "<< /Type /Page /Parent 2 0 R >>"),
        (5, "<< /Type /Page /Parent 2 0 R >>"),
        (6, "<< /Type /StructElem /S /P /P 10 0 R /K 0 /Pg 4 0 R >>"),
        (
            7,
            "<< /Type /StructElem /S /Sect /P 10 0 R /K << /Type /MCR /Pg 3 0 R /MCID 1 >> >>",
        ),
        (9, "<< /Type /StructElem /S /Figure /P 10 0 R /K [] >>"),
        (10, "<< /Type /StructTreeRoot /K [6 0 R 7 0 R 9 0 R] >>"),
        (
            11,
            "<< /Type /Outlines /First 14 0 R /Last 14 0 R /Count 1 >>",
        ),
        (
            12,
            "<< /Type /Annot /Subtype /Link /Rect [0 0 9 9] /Dest [6 0 R /XYZ 0 100 0] >>",
        ),
        (
            13,
            "<< /Type /Annot /Subtype /Link /Rect [0 0 9 9] /A << /S /GoTo /SD [7 0 R /Fit] >> >>",
        ),
        (
            14,
            "<< /Title (Body) /Parent 11 0 R /Dest [6 0 R /FitH 50] >>",
        ),
        (
            15,
            "<< /Type /Annot /Subtype /Link /Rect [0 0 9 9] /Dest [9 0 R /Fit] >>",
        ),
    ])
}

fn view(doc: &Document) -> DocumentView<'_> {
    DocumentView::new(doc, doc.bytes(), doc.version())
}

/// The output's page objects in order, and its annotations on page 1.
fn open(bytes: Vec<u8>) -> (Document, Vec<ObjId>, Vec<Dict>) {
    let text = String::from_utf8_lossy(&bytes).into_owned();
    assert!(
        !text.contains("/StructElem"),
        "a structure element was copied"
    );
    assert!(!text.contains("/SD"), "a structure destination was copied");
    let out = Document::from_bytes(bytes).unwrap();
    let pages: Vec<ObjId> = crate::page_tree::pages(&out)
        .unwrap()
        .iter()
        .map(|p| p.id)
        .collect();
    let page = out.value(pages[0]).and_then(Object::as_dict).unwrap();
    let annots = page
        .get(b"Annots")
        .and_then(Object::as_array)
        .unwrap()
        .iter()
        .map(|a| out.resolve(a).as_dict().unwrap().clone())
        .collect();
    (out, pages, annots)
}

fn first(dest: &Object) -> ObjId {
    dest.as_array().unwrap()[0].as_reference().unwrap()
}

#[test]
fn extracting_a_tagged_file_points_its_structure_links_at_pages() {
    let doc = tagged();
    let (bytes, report) = extract(&view(&doc), &[0, 1]).unwrap();
    assert_eq!(report.dangling_references, 0);
    assert!(report.struct_tree_dropped);
    let (out, pages, annots) = open(bytes);
    let dest = annots[0].get(b"Dest").unwrap();
    assert_eq!(first(dest), pages[1], "element 6 marks page 4");
    assert_eq!(dest.as_array().unwrap().len(), 5, "operands kept");
    let goto = annots[1].get(b"A").and_then(Object::as_dict).unwrap();
    assert_eq!(
        first(goto.get(b"D").unwrap()),
        pages[0],
        "/D built from /SD"
    );
    assert_eq!(
        first(annots[2].get(b"Dest").unwrap()),
        pages[0],
        "no page: the source's first"
    );
    let outlines = out
        .resolve(out.catalog().unwrap().get(b"Outlines").unwrap())
        .as_dict()
        .unwrap()
        .clone();
    let item = out
        .resolve(outlines.get(b"First").unwrap())
        .as_dict()
        .unwrap();
    assert_eq!(first(item.get(b"Dest").unwrap()), pages[1]);
}

#[test]
fn a_structure_link_to_a_page_left_behind_is_counted_dangling() {
    let doc = tagged();
    let (bytes, report) = extract(&view(&doc), &[0]).unwrap();
    assert_eq!(report.dangling_references, 1);
    assert_eq!(report.outline_items_dropped, 1);
    let (_, _, annots) = open(bytes);
    assert!(annots[0].get(b"Dest").is_none());
    assert!(
        annots[1]
            .get(b"A")
            .and_then(Object::as_dict)
            .unwrap()
            .get(b"D")
            .is_some()
    );
}

#[test]
fn merging_two_tagged_files_points_each_at_its_own_pages() {
    let (a, b) = (tagged(), tagged());
    let (bytes, report) = merge(&[view(&a), view(&b)], &[], &[]).unwrap();
    assert_eq!(report.structure_links_to_pages, 6);
    assert_eq!(report.structure_links_to_first_page, 2);
    assert_eq!(report.dangling_references, 0);
    let (out, pages, annots) = open(bytes);
    assert_eq!(first(annots[0].get(b"Dest").unwrap()), pages[1]);
    let second = out.value(pages[3]).and_then(Object::as_dict).unwrap();
    let link = out.resolve(&second.get(b"Annots").unwrap().as_array().unwrap()[0]);
    let dest = link.as_dict().unwrap().get(b"Dest").unwrap();
    assert_eq!(first(dest), pages[4], "the second file's element 6");
}

#[test]
fn a_copied_page_does_not_drag_its_old_parent_node_along() {
    let doc = tagged();
    let (bytes, report) = extract(&view(&doc), &[1]).unwrap();
    assert_eq!(report.dangling_references, 0);
    let text = String::from_utf8_lossy(&bytes);
    assert_eq!(text.matches("/Type /Pages").count(), 1, "{text}");
}
