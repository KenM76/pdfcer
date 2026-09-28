//! `pdfcer_text::structure_tree` (G053) against synthetic tagged files.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_model::document::Document;
use pdfcer_text::structure_tree::{StructKid, StructTreatment, StructureTree, read_structure_tree};
use pdfcer_text::text_extract::{ContentStreamRef, ExtractOptions};

/// A classic-xref PDF whose object `i + 1` is `bodies[i]`. A body starting
/// with `STREAM:` becomes a stream with that payload; `FORM:`, a form
/// XObject using font `5 0 R`.
fn build_pdf(bodies: &[&str]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        let body = match body.strip_prefix("STREAM:") {
            Some(data) => format!("<< /Length {} >>\nstream\n{data}\nendstream", data.len()),
            None => match body.strip_prefix("FORM:") {
                Some(data) => format!(
                    "<< /Type /XObject /Subtype /Form /BBox [0 0 612 792] \
                     /Resources << /Font << /F1 5 0 R >> >> /Length {} >>\nstream\n{data}\nendstream",
                    data.len()
                ),
                None => (*body).to_owned(),
            },
        };
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

fn read(bytes: Vec<u8>) -> StructureTree {
    let doc = Document::from_bytes(bytes).unwrap();
    read_structure_tree(&doc.view(), &ExtractOptions::default()).unwrap()
}

const CONTENT: &str = "STREAM:\
/H1 <</MCID 0>> BDC BT /F1 12 Tf 72 700 Td (Title) Tj ET EMC\n\
/P <</MCID 1>> BDC BT /F1 12 Tf 72 680 Td (Body text) Tj ET EMC\n\
/Figure <</MCID 2>> BDC 0 0 m 10 10 l S EMC\n\
/TD <</MCID 4>> BDC BT /F1 12 Tf 72 600 Td (Cell) Tj ET EMC\n\
/Span <</MCID 3>> BDC BT /F1 12 Tf 72 500 Td (orphan) Tj ET EMC";

const PAGE: &str = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
                    /Resources << /Font << /F1 5 0 R >> >> >>";
const FONT: &str = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>";

/// Catalog + page + content + font, then the structure objects from 6 on.
fn tagged(structure: &[&str]) -> Vec<u8> {
    let mut bodies = vec![
        "<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 6 0 R /Lang (en-US) \
         /MarkInfo << /Marked true >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        PAGE,
        CONTENT,
        FONT,
    ];
    bodies.extend_from_slice(structure);
    build_pdf(&bodies)
}

fn word_like() -> StructureTree {
    read(tagged(&[
        // 6: root
        "<< /Type /StructTreeRoot /K 7 0 R /RoleMap << /Heading1 /H1 >> \
         /ClassMap << /wide << /O /Table /ColSpan 3 >> >> >>",
        // 7: Document
        "<< /Type /StructElem /S /Document /P 6 0 R /K [8 0 R 9 0 R 10 0 R 11 0 R] >>",
        // 8: a producer's own heading type, role mapped
        "<< /S /Heading1 /P 7 0 R /Pg 3 0 R /K 0 >>",
        // 9: a paragraph reached through an /MCR, with its own /Lang
        "<< /S /P /P 7 0 R /Pg 3 0 R /Lang (fr-CA) /K << /Type /MCR /Pg 3 0 R /MCID 1 >> >>",
        // 10: a figure whose content paints no text, plus a dangling MCID
        "<< /S /Figure /P 7 0 R /Pg 3 0 R /Alt (A line) /K [2 9] >>",
        // 11-13: a table; the row also points back at the Document
        "<< /S /Table /P 7 0 R /Pg 3 0 R /K 12 0 R >>",
        "<< /S /TR /P 11 0 R /K [13 0 R 7 0 R] >>",
        "<< /S /TD /P 12 0 R /A << /O /Table /RowSpan 2 >> /C /wide /K 4 >>",
    ]))
}

#[test]
fn elements_come_back_in_logical_order_with_mapped_types() {
    let t = word_like();
    let types: Vec<&str> = t
        .elements
        .iter()
        .map(|e| e.resolved_type.as_str())
        .collect();
    assert_eq!(
        types,
        ["Document", "H1", "P", "Figure", "Table", "TR", "TD"]
    );
    assert_eq!(t.roots, [0]);
    let h = &t.elements[1];
    assert_eq!(h.raw_type, "Heading1");
    assert!(h.standard);
    assert_eq!(h.parent, Some(0));
    assert_eq!(h.depth, 1);
    assert_eq!(t.elements[6].depth, 3);
    assert_eq!(t.diagnostics.non_standard_types, 0);
}

#[test]
fn element_text_joins_through_mcids_and_mcrs() {
    let t = word_like();
    assert_eq!(t.element_text(1), "Title");
    assert_eq!(t.element_text(2), "Body text");
    assert_eq!(t.element_text(6), "Cell");
    assert_eq!(t.element_text(0), "Title Body text Cell");
    let (page, bbox) = t.element_bbox(1)[0];
    assert_eq!(page, 0);
    assert!(bbox.lly >= 695.0 && bbox.lly <= 701.0, "{bbox:?}");
}

#[test]
fn a_broken_tree_is_counted_not_trusted() {
    let t = word_like();
    let d = &t.diagnostics;
    assert!(d.struct_tree_present);
    assert_eq!(d.mcids_named, 5);
    assert_eq!(d.named_not_declared, 1, "MCID 9 is never declared");
    assert_eq!(d.declared_unclaimed, 1, "MCID 3 belongs to no element");
    assert_eq!(d.elements_revisited, 1, "the row points back at Document");
    assert_eq!(d.page_inherited, 1, "the TD has no /Pg of its own");
    // The figure's sequence paints a path: declared, but no text runs.
    let fig = &t.elements[3];
    assert_eq!(fig.alt.as_deref(), Some("A line"));
    match &fig.kids[0] {
        StructKid::MarkedContent {
            mcid: 2,
            declared: true,
            runs,
            stream: ContentStreamRef::Page,
            page_index: Some(0),
        } => assert!(runs.is_empty()),
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        fig.kids[1],
        StructKid::MarkedContent {
            mcid: 9,
            declared: false,
            ..
        }
    ));
}

#[test]
fn table_cell_attributes_resolve_from_a_and_class_map() {
    let t = word_like();
    let td = &t.elements[6];
    assert_eq!(td.row_span, Some(2));
    assert_eq!(td.col_span, Some(3));
    assert_eq!(t.elements[5].row_span, None, "only TH/TD carry spans");
}

#[test]
fn lang_inherits_from_the_catalog_and_ancestors() {
    let t = word_like();
    assert_eq!(t.elements[1].effective_lang.as_deref(), Some("en-US"));
    assert_eq!(t.elements[2].lang.as_deref(), Some("fr-CA"));
    assert_eq!(t.elements[2].effective_lang.as_deref(), Some("fr-CA"));
    assert_eq!(t.elements[1].lang, None);
}

#[test]
fn actual_text_replaces_the_elements_content() {
    let t = read(tagged(&[
        "<< /Type /StructTreeRoot /K [7 0 R 8 0 R] >>",
        "<< /S /P /P 6 0 R /Pg 3 0 R /ActualText (Heading) /K 0 >>",
        "<< /S /P /P 6 0 R /Pg 3 0 R /K 1 >>",
    ]));
    assert_eq!(t.roots, [0, 1]);
    assert_eq!(t.element_text(0), "Heading");
    assert_eq!(t.element_text(1), "Body text");
}

#[test]
fn a_standard_name_in_the_role_map_is_still_remapped() {
    let t = read(tagged(&[
        "<< /Type /StructTreeRoot /K 7 0 R /RoleMap << /P /H2 >> >>",
        "<< /S /P /P 6 0 R /Pg 3 0 R /K 0 >>",
    ]));
    assert_eq!(t.elements[0].raw_type, "P");
    assert_eq!(t.elements[0].resolved_type, "H2");
}

#[test]
fn a_role_map_cycle_stops_and_is_non_standard() {
    let t = read(tagged(&[
        "<< /Type /StructTreeRoot /K 7 0 R /RoleMap << /Foo /Bar /Bar /Foo >> >>",
        "<< /S /Foo /P 6 0 R /Pg 3 0 R /K 0 >>",
    ]));
    let e = &t.elements[0];
    assert!(!e.standard);
    assert_eq!(e.raw_type, "Foo");
    assert_eq!(t.diagnostics.role_map_cycles, 1);
    assert_eq!(t.diagnostics.non_standard_types, 1);
}

#[test]
fn pdf_2_namespaces_map_through_role_map_ns() {
    let t = read(tagged(&[
        "<< /Type /StructTreeRoot /K [7 0 R 8 0 R 9 0 R 10 0 R] /Namespaces [11 0 R 12 0 R] >>",
        "<< /S /Aside /NS 11 0 R /P 6 0 R /Pg 3 0 R /K 0 >>",
        "<< /S /H7 /NS 11 0 R /P 6 0 R /Pg 3 0 R /K 1 >>",
        "<< /S /MyHead /NS 12 0 R /P 6 0 R /Pg 3 0 R /K 4 >>",
        "<< /S /Aside /P 6 0 R /Pg 3 0 R /K 3 >>",
        "<< /Type /Namespace /NS (http://iso.org/pdf2/ssn) >>",
        "<< /Type /Namespace /NS (urn:example) /RoleMapNS << /MyHead [/Title 11 0 R] >> >>",
    ]));
    let e = &t.elements;
    assert!(e[0].standard);
    assert_eq!(e[0].namespace.as_deref(), Some("http://iso.org/pdf2/ssn"));
    assert!(e[1].standard, "Hn with n > 6 is a PDF 2.0 type");
    assert_eq!(e[2].resolved_type, "Title");
    assert!(e[2].standard);
    assert!(
        !e[3].standard,
        "Aside is not a type of the default (PDF 1.7) namespace"
    );
    assert_eq!(e[3].treatment, StructTreatment::Normal);
}

#[test]
fn artifact_and_private_subtrees_are_left_out_of_text() {
    let t = read(tagged(&[
        "<< /Type /StructTreeRoot /K 7 0 R /Namespaces [10 0 R] >>",
        "<< /S /Div /P 6 0 R /Pg 3 0 R /K [0 8 0 R 9 0 R] >>",
        "<< /S /Artifact /NS 10 0 R /P 7 0 R /Pg 3 0 R /K 1 >>",
        "<< /S /Private /P 7 0 R /Pg 3 0 R /K 4 >>",
        "<< /Type /Namespace /NS (http://iso.org/pdf2/ssn) >>",
    ]));
    assert_eq!(t.elements[1].treatment, StructTreatment::Artifact);
    assert_eq!(t.elements[2].treatment, StructTreatment::Private);
    assert_eq!(t.element_text(0), "Title");
}

#[test]
fn an_untagged_file_is_not_an_error() {
    let t = read(build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        PAGE,
        CONTENT,
        FONT,
    ]));
    assert!(!t.diagnostics.struct_tree_present);
    assert!(t.elements.is_empty());
    assert_eq!(t.diagnostics.declared_unclaimed, 0);
    assert_eq!(t.text.pages.len(), 1);
    assert_eq!(t.text.pages[0].marked_content_ids.len(), 5);
}

#[test]
fn a_form_xobjects_mcids_are_a_separate_key_from_the_pages() {
    let t = read(build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 7 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R >> /XObject << /X1 6 0 R >> >> >>",
        "STREAM:/P <</MCID 0>> BDC BT /F1 12 Tf 72 700 Td (Page) Tj ET EMC /X1 Do",
        FONT,
        "FORM:/Span <</MCID 0>> BDC BT /F1 12 Tf 72 650 Td (Form) Tj ET EMC",
        "<< /Type /StructTreeRoot /K [8 0 R 9 0 R] >>",
        "<< /S /P /P 7 0 R /Pg 3 0 R /K 0 >>",
        "<< /S /Span /P 7 0 R /K << /Type /MCR /Pg 3 0 R /Stm 6 0 R /MCID 0 >> >>",
    ]));
    assert_eq!(t.element_text(0), "Page");
    assert_eq!(t.element_text(1), "Form");
    assert_eq!(t.diagnostics.named_not_declared, 0);
    assert_eq!(t.diagnostics.declared_unclaimed, 0);
    assert_eq!(t.diagnostics.claimed_twice, 0);
}
