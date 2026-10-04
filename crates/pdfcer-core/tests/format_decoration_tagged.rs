//! A decoration on a tagged page is recorded in the structure tree as
//! `/TextDecorationType` on each element whose text it fully covers, and
//! disclosed otherwise (decision 188).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::text_edit::decoration::DecorationSet;
use pdfcer_core::text_edit::{FormatOptions, FormatReport, FormatRequest};
use pdfcer_core::writer::SaveOptions;

/// MCID 0 holds "Hello", MCID 1 holds "World" and "Again"; "Loose" is
/// untagged.
const CONTENT: &str = "BT /F1 12 Tf /P <</MCID 0>> BDC 1 0 0 1 72 700 Tm (Hello) Tj EMC \
     /P <</MCID 1>> BDC 1 0 0 1 200 700 Tm (World) Tj 1 0 0 1 300 700 Tm (Again) Tj EMC \
     1 0 0 1 72 600 Tm (Loose) Tj ET";

/// A tagged one-page document: element 9 owns MCID 0, element 10 owns
/// MCID 1; `first` is element 9's extra entries, `extra` more objects
/// from 11.
fn pdf(first: &str, extra: &[&str]) -> Vec<u8> {
    pdf_with(first, "", extra)
}

/// [`pdf`] with `second` as element 10's extra entries.
fn pdf_with(first: &str, second: &str, extra: &[&str]) -> Vec<u8> {
    pdf_tree(first, second, "<< /Nums [0 [9 0 R 10 0 R]] >>", extra)
}

/// [`pdf_with`] with `tree` as the parent tree (object 8).
fn pdf_tree(first: &str, second: &str, tree: &str, extra: &[&str]) -> Vec<u8> {
    let widths = (0..95).map(|_| "500").collect::<Vec<_>>().join(" ");
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 6 0 R /MarkInfo << /Marked true >> >>"
            .to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 5 0 R \
         /StructParents 0 /Resources << /Font << /F1 4 0 R >> >> >>"
            .to_owned(),
        format!(
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
             /Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 126 /Widths [{widths}] >>"
        ),
        format!(
            "<< /Length {} >>\nstream\n{CONTENT}\nendstream",
            CONTENT.len()
        ),
        "<< /Type /StructTreeRoot /K 7 0 R /ParentTree 8 0 R >>".to_owned(),
        "<< /Type /StructElem /S /Document /P 6 0 R /K [9 0 R 10 0 R] >>".to_owned(),
        tree.to_owned(),
        format!(
            "<< /Type /StructElem /S /P /P 7 0 R /Pg 3 0 R {} >>",
            kids(first)
        ),
        format!("<< /Type /StructElem /S /P /P 7 0 R /Pg 3 0 R /K 1 {second} >>"),
    ];
    objects.extend(extra.iter().map(|s| (*s).to_owned()));
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// Element 9's entries: `first`, after `/K 0` unless `first` sets `/K`.
fn kids(first: &str) -> String {
    if first.contains("/K ") {
        first.to_owned()
    } else {
        format!("/K 0 {first}")
    }
}

fn session(first: &str, extra: &[&str]) -> EditSession {
    EditSession::new(Document::from_bytes(pdf(first, extra)).unwrap())
}

fn decorate(s: &mut EditSession, find: &str, set: DecorationSet) -> FormatReport {
    let req = FormatRequest::new(0, find).decoration(set);
    s.format_text(&req, &FormatOptions::default()).unwrap()
}

/// Element `num`'s `/A` after a save and reopen; `None` when absent.
fn saved_a(s: &EditSession, num: u32) -> Option<Object> {
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let doc = Document::from_bytes(bytes).unwrap();
    let view = doc.view();
    let element = view.value(ObjId::new(num, 0)).unwrap().as_dict().unwrap();
    element.get(b"A").cloned()
}

fn decoration_of(a: &Object) -> Option<Vec<u8>> {
    let dicts: Vec<&pdfcer_core::object::Dict> = match a {
        Object::Array(items) => items.iter().filter_map(Object::as_dict).collect(),
        other => other.as_dict().into_iter().collect(),
    };
    dicts
        .iter()
        .find_map(|d| d.get(b"TextDecorationType").and_then(Object::as_name))
        .map(|n| n.as_bytes().to_vec())
}

fn has_note(report: &FormatReport, needle: &str) -> bool {
    report.disclosures.iter().any(|d| d.contains(needle))
}

#[test]
fn a_fully_covered_element_records_the_underline() {
    let mut s = session("", &[]);
    let report = decorate(&mut s, "Hello", DecorationSet::UNDERLINE);
    let a = saved_a(&s, 9).expect("element 9 gains /A");
    let d = a.as_dict().unwrap();
    assert_eq!(
        d.get(b"O").and_then(Object::as_name).unwrap().as_bytes(),
        b"Layout"
    );
    assert_eq!(decoration_of(&a).as_deref(), Some(&b"Underline"[..]));
    assert!(saved_a(&s, 10).is_none(), "the other element is untouched");
    assert!(
        !has_note(&report, "pdfcer does not split"),
        "{:?}",
        report.disclosures
    );
}

#[test]
fn a_partly_covered_element_is_disclosed_not_recorded() {
    let mut s = session("", &[]);
    let report = decorate(&mut s, "World", DecorationSet::UNDERLINE);
    assert!(saved_a(&s, 10).is_none());
    assert!(
        has_note(&report, "covers part of a <P> element"),
        "{:?}",
        report.disclosures
    );
}

#[test]
fn undo_returns_the_element_to_its_base_value() {
    let mut s = session("", &[]);
    decorate(&mut s, "Hello", DecorationSet::UNDERLINE);
    s.undo().unwrap();
    assert!(!s.is_modified(), "the dirty set is net-zero after undo");
    let id = ObjId::new(9, 0);
    assert!(
        s.view()
            .value(id)
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"A")
            .is_none()
    );
}

#[test]
fn clearing_the_decoration_removes_the_recorded_attribute() {
    let mut s = session("", &[]);
    decorate(&mut s, "Hello", DecorationSet::UNDERLINE);
    decorate(&mut s, "Hello", DecorationSet::NONE);
    assert!(saved_a(&s, 9).is_none());
}

#[test]
fn line_through_wins_and_the_underline_is_disclosed() {
    let mut s = session("", &[]);
    let both = DecorationSet::UNDERLINE.with_strikethrough(true);
    let report = decorate(&mut s, "Hello", both);
    let a = saved_a(&s, 9).unwrap();
    assert_eq!(decoration_of(&a).as_deref(), Some(&b"LineThrough"[..]));
    assert!(
        has_note(&report, "the underline is not recorded"),
        "{:?}",
        report.disclosures
    );
}

#[test]
fn untagged_text_is_disclosed() {
    let mut s = session("", &[]);
    let report = decorate(&mut s, "Loose", DecorationSet::UNDERLINE);
    assert!(
        has_note(&report, "this text is not tagged"),
        "{:?}",
        report.disclosures
    );
}

#[test]
fn an_indirect_attribute_object_is_never_mutated() {
    let mut s = session("/A 11 0 R", &["<< /O /Layout /TextAlign /Center >>"]);
    decorate(&mut s, "Hello", DecorationSet::UNDERLINE);
    let a = saved_a(&s, 9).unwrap();
    let items = a.as_array().expect("/A becomes an array");
    assert_eq!(items[0].as_reference(), Some(ObjId::new(11, 0)));
    assert_eq!(decoration_of(&a).as_deref(), Some(&b"Underline"[..]));
    let attr = s
        .view()
        .value(ObjId::new(11, 0))
        .unwrap()
        .as_dict()
        .unwrap()
        .clone();
    assert!(attr.get(b"TextDecorationType").is_none());
}

#[test]
fn a_direct_layout_dictionary_is_merged() {
    let mut s = session("/A << /O /Layout /TextAlign /Center >>", &[]);
    decorate(&mut s, "Hello", DecorationSet::UNDERLINE);
    let a = saved_a(&s, 9).unwrap();
    let d = a.as_dict().expect("still one dictionary");
    assert!(d.get(b"TextAlign").is_some());
    assert_eq!(decoration_of(&a).as_deref(), Some(&b"Underline"[..]));
}

#[test]
fn a_revised_element_gets_its_revision_after_the_new_attribute() {
    let mut s = session("/R 2", &[]);
    decorate(&mut s, "Hello", DecorationSet::UNDERLINE);
    let a = saved_a(&s, 9).unwrap();
    let items = a.as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[1].as_int(), Some(2));
}

#[test]
fn an_authored_attribute_that_now_over_claims_is_disclosed() {
    // Element 10 already says underlined; only part of its text is.
    let second = "/A << /O /Layout /TextDecorationType /Underline >>";
    let mut s = EditSession::new(Document::from_bytes(pdf_with("", second, &[])).unwrap());
    let report = decorate(&mut s, "World", DecorationSet::UNDERLINE);
    assert!(
        has_note(&report, "still marks a <P> element on page 1 as underline"),
        "{:?}",
        report.disclosures
    );
}

#[test]
fn a_nested_inline_element_is_part_of_its_parents_text() {
    let mut s = session(
        "/K [0 11 0 R]",
        &["<< /Type /StructElem /S /Span /P 9 0 R /K 1 >>"],
    );
    let report = decorate(&mut s, "Hello", DecorationSet::UNDERLINE);
    assert!(
        saved_a(&s, 9).is_none(),
        "the Span's text is not underlined"
    );
    assert!(
        has_note(&report, "covers part of a <P> element"),
        "{:?}",
        report.disclosures
    );
}

#[test]
fn a_nested_block_element_is_not_part_of_its_parents_text() {
    let mut s = session(
        "/K [0 11 0 R]",
        &["<< /Type /StructElem /S /P /P 9 0 R /K 1 >>"],
    );
    decorate(&mut s, "Hello", DecorationSet::UNDERLINE);
    let a = saved_a(&s, 9).expect("the nested paragraph does not count");
    assert_eq!(decoration_of(&a).as_deref(), Some(&b"Underline"[..]));
}

#[test]
fn a_marked_content_reference_counts_toward_coverage() {
    let mut s = session("/K [0 << /Type /MCR /Pg 3 0 R /MCID 1 >>]", &[]);
    decorate(&mut s, "Hello", DecorationSet::UNDERLINE);
    assert!(saved_a(&s, 9).is_none());
}

#[test]
fn the_parent_tree_is_searched_through_its_kids() {
    let tree = "<< /Kids [11 0 R 12 0 R] >>";
    let extra = [
        // Out of range by its /Limits, so never read; it would hide element 9.
        "<< /Limits [5 9] /Nums [0 [10 0 R]] >>",
        "<< /Limits [0 4] /Kids [12 0 R 13 0 R] >>",
        "<< /Limits [0 0] /Nums [0 [9 0 R 10 0 R]] >>",
    ];
    let bytes = pdf_tree("", "", tree, &extra);
    let mut s = EditSession::new(Document::from_bytes(bytes).unwrap());
    decorate(&mut s, "Hello", DecorationSet::UNDERLINE);
    assert!(
        saved_a(&s, 9).is_some(),
        "found past an out-of-range and a cyclic kid"
    );
}
