//! `pdfcer_text::structure_bounds` (Pass 706.0, request X076) against
//! synthetic tagged files: painted bounds per element, including paths,
//! images, shadings, clips, forms and `/OBJR`s, plus the no-bounds report.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_model::document::Document;
use pdfcer_model::page_tree::Rect;
use pdfcer_text::structure_bounds::{NoBoundsReason, PaintedBounds, read_painted_bounds};
use pdfcer_text::structure_tree::{StructureTree, read_structure_tree};
use pdfcer_text::text_extract::{ContentStreamRef, ExtractOptions};

/// A classic-xref PDF whose object `i + 1` is `bodies[i]`. `STREAM:<data>`
/// is a plain stream; `XSTREAM:<dict entries>|<data>` a stream whose
/// dictionary also carries those entries.
fn build_pdf(bodies: &[String]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        let body = if let Some(data) = body.strip_prefix("STREAM:") {
            format!("<< /Length {} >>\nstream\n{data}\nendstream", data.len())
        } else if let Some(rest) = body.strip_prefix("XSTREAM:") {
            let (dict, data) = rest.split_once('|').unwrap();
            format!(
                "<< {dict} /Length {} >>\nstream\n{data}\nendstream",
                data.len()
            )
        } else {
            body.clone()
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

const FONT: &str = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>";
const RES: &str = "/Resources << /Font << /F1 5 0 R >> /XObject << /Im1 6 0 R /Fm1 7 0 R >> >>";
const IMAGE: &str = "XSTREAM:/Type /XObject /Subtype /Image /Width 1 /Height 1 \
                     /ColorSpace /DeviceGray /BitsPerComponent 8|A";

/// Objects 1–7 are fixed: catalog, pages, page 3 (content 4), font 5,
/// image 6, form 7 (content `form`). A second page, when `page2` is
/// `Some`, is object 8 with content object 9. The structure root is
/// object 10, then `structure` from 11 on.
fn tagged(page1: &str, form: &str, page2: Option<&str>, structure: &[&str]) -> Vec<u8> {
    let kids = if page2.is_some() {
        "[3 0 R 8 0 R] /Count 2"
    } else {
        "[3 0 R] /Count 1"
    };
    let mut bodies = vec![
        "<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 10 0 R >>".to_owned(),
        format!("<< /Type /Pages /Kids {kids} >>"),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R {RES} >>"),
        format!("STREAM:{page1}"),
        FONT.to_owned(),
        IMAGE.to_owned(),
        format!(
            "XSTREAM:/Type /XObject /Subtype /Form /BBox [0 0 612 792] \
             /Resources << /Font << /F1 5 0 R >> /XObject << /Fm1 7 0 R >> >>|{form}"
        ),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 9 0 R {RES} >>"),
        format!("STREAM:{}", page2.unwrap_or("")),
    ];
    bodies.extend(structure.iter().map(|s| (*s).to_owned()));
    build_pdf(&bodies)
}

fn read(bytes: Vec<u8>) -> (StructureTree, PaintedBounds) {
    let doc = Document::from_bytes(bytes).unwrap();
    let view = doc.view();
    let tree = read_structure_tree(&view, &ExtractOptions::default()).unwrap();
    let painted = read_painted_bounds(&view, &tree).unwrap();
    (tree, painted)
}

fn rect(llx: f64, lly: f64, urx: f64, ury: f64) -> Rect {
    Rect::from_corners(llx, lly, urx, ury)
}

fn assert_rect(got: Rect, want: Rect) {
    let close = |a: f64, b: f64| (a - b).abs() < 1e-6;
    assert!(
        close(got.llx, want.llx)
            && close(got.lly, want.lly)
            && close(got.urx, want.urx)
            && close(got.ury, want.ury),
        "got {got:?}, want {want:?}"
    );
}

/// The one rect `element` paints on `page`.
fn only(tree: &StructureTree, painted: &PaintedBounds, element: usize, page: usize) -> Rect {
    let b = tree.element_painted_bounds(element, painted);
    assert_eq!(b.len(), 1, "{b:?}");
    assert_eq!(b[0].0, page);
    b[0].1
}

fn cell(mcid: u32, x: u32, y: u32, text: &str) -> String {
    format!(
        "/TD <</MCID {mcid}>> BDC 0.9 g {x} {y} 100 50 re f 0 g \
         BT /F1 12 Tf {} {} Td ({text}) Tj ET EMC\n",
        x + 10,
        y + 20
    )
}

/// 11 Table, 12–13 TR, 14–17 TD (elements 0, 1, 4 and 2, 3, 5, 6).
fn table_2x2() -> Vec<u8> {
    let page = [
        cell(0, 100, 600, "A"),
        cell(1, 200, 600, "B"),
        cell(2, 100, 550, "C"),
        cell(3, 200, 550, "D"),
    ]
    .concat();
    tagged(
        &page,
        "",
        None,
        &[
            "<< /Type /StructTreeRoot /K 11 0 R >>",
            "<< /S /Table /P 10 0 R /Pg 3 0 R /K [12 0 R 13 0 R] >>",
            "<< /S /TR /P 11 0 R /Pg 3 0 R /K [14 0 R 15 0 R] >>",
            "<< /S /TR /P 11 0 R /Pg 3 0 R /K [16 0 R 17 0 R] >>",
            "<< /S /TD /P 12 0 R /Pg 3 0 R /K 0 >>",
            "<< /S /TD /P 12 0 R /Pg 3 0 R /K 1 >>",
            "<< /S /TD /P 13 0 R /Pg 3 0 R /K 2 >>",
            "<< /S /TD /P 13 0 R /Pg 3 0 R /K 3 >>",
        ],
    )
}

#[test]
fn a_table_cell_is_its_filled_rectangle_and_rows_and_table_are_unions() {
    let (t, p) = read(table_2x2());
    let types: Vec<&str> = t
        .elements
        .iter()
        .map(|e| e.resolved_type.as_str())
        .collect();
    assert_eq!(types, ["Table", "TR", "TD", "TD", "TR", "TD", "TD"]);
    assert_rect(only(&t, &p, 2, 0), rect(100.0, 600.0, 200.0, 650.0));
    assert_rect(only(&t, &p, 3, 0), rect(200.0, 600.0, 300.0, 650.0));
    assert_rect(only(&t, &p, 6, 0), rect(200.0, 550.0, 300.0, 600.0));
    assert_rect(only(&t, &p, 1, 0), rect(100.0, 600.0, 300.0, 650.0));
    assert_rect(only(&t, &p, 4, 0), rect(100.0, 550.0, 300.0, 600.0));
    assert_rect(only(&t, &p, 0, 0), rect(100.0, 550.0, 300.0, 650.0));
    assert!(t.elements_without_painted_bounds(&p).is_empty());
}

#[test]
fn element_bbox_keeps_its_glyph_hull() {
    let (t, _) = read(table_2x2());
    let b = t.element_bbox(2);
    assert_eq!(b.len(), 1);
    let (page, hull) = b[0];
    assert_eq!(page, 0);
    // The run's own box, strictly inside the cell rectangle.
    let run = t.text.pages[0]
        .runs
        .iter()
        .find(|r| r.text == "A")
        .and_then(|r| r.bbox)
        .unwrap();
    assert_eq!(hull, run);
    assert!(hull.llx > 100.0 && hull.urx < 200.0 && hull.lly > 600.0 && hull.ury < 650.0);
}

#[test]
fn an_image_only_cell_is_the_image_placement_and_objr_to_the_image_is_too() {
    let (t, p) = read(tagged(
        "/TD <</MCID 0>> BDC q 40 0 0 30 100 100 cm /Im1 Do Q EMC",
        "",
        None,
        &[
            "<< /Type /StructTreeRoot /K [11 0 R 12 0 R] >>",
            "<< /S /TD /P 10 0 R /Pg 3 0 R /K 0 >>",
            "<< /S /Figure /P 10 0 R /K << /Type /OBJR /Obj 6 0 R /Pg 3 0 R >> >>",
        ],
    ));
    let placed = rect(100.0, 100.0, 140.0, 130.0);
    assert_rect(only(&t, &p, 0, 0), placed);
    assert!(t.element_bbox(0).is_empty(), "no text, no glyph hull");
    assert_rect(only(&t, &p, 1, 0), placed);
}

#[test]
fn a_table_across_two_pages_has_one_rect_per_page() {
    let (t, p) = read(tagged(
        &cell(0, 100, 600, "A"),
        "",
        Some(&cell(0, 50, 100, "B")),
        &[
            "<< /Type /StructTreeRoot /K 11 0 R >>",
            "<< /S /Table /P 10 0 R /K [12 0 R 13 0 R] >>",
            "<< /S /TR /P 11 0 R /Pg 3 0 R /K 14 0 R >>",
            "<< /S /TR /P 11 0 R /Pg 8 0 R /K 15 0 R >>",
            "<< /S /TD /P 12 0 R /Pg 3 0 R /K 0 >>",
            "<< /S /TD /P 13 0 R /Pg 8 0 R /K 0 >>",
        ],
    ));
    let b = t.element_painted_bounds(0, &p);
    assert_eq!(b.len(), 2, "{b:?}");
    assert_eq!((b[0].0, b[1].0), (0, 1));
    assert_rect(b[0].1, rect(100.0, 600.0, 200.0, 650.0));
    assert_rect(b[1].1, rect(50.0, 100.0, 150.0, 150.0));
}

#[test]
fn a_form_mcid_and_a_page_mcid_of_the_same_number_are_different_boxes() {
    let (t, p) = read(tagged(
        "/P <</MCID 0>> BDC 10 10 10 10 re f EMC q 1 0 0 1 100 0 cm /Fm1 Do Q",
        "/P <</MCID 0>> BDC 300 400 50 50 re f EMC",
        None,
        &[
            "<< /Type /StructTreeRoot /K [11 0 R 12 0 R] >>",
            "<< /S /P /P 10 0 R /Pg 3 0 R /K 0 >>",
            "<< /S /P /P 10 0 R /K << /Type /MCR /Pg 3 0 R /Stm 7 0 R /MCID 0 >> >>",
        ],
    ));
    assert_rect(only(&t, &p, 0, 0), rect(10.0, 10.0, 20.0, 20.0));
    assert_rect(only(&t, &p, 1, 0), rect(400.0, 400.0, 450.0, 450.0));
    assert_rect(
        p.sequence(0, ContentStreamRef::Form { object: 7 }, 0)
            .unwrap(),
        rect(400.0, 400.0, 450.0, 450.0),
    );
}

#[test]
fn a_link_objr_keeps_the_annotation_rect() {
    let (t, p) = read(tagged(
        "",
        "",
        None,
        &[
            "<< /Type /StructTreeRoot /K 11 0 R >>",
            "<< /S /Link /P 10 0 R /Pg 3 0 R /K << /Type /OBJR /Obj 12 0 R >> >>",
            "<< /Type /Annot /Subtype /Link /Rect [150 720 50 700] >>",
        ],
    ));
    assert_rect(only(&t, &p, 0, 0), rect(50.0, 700.0, 150.0, 720.0));
}

#[test]
fn strokes_widen_by_half_the_line_width_under_the_ctm() {
    let (t, p) = read(tagged(
        "/Figure <</MCID 0>> BDC 2 w 50 50 m 150 50 l S EMC\n\
         q 2 0 0 2 0 0 cm /Figure <</MCID 1>> BDC 2 w 100 100 m 100 150 l S EMC Q",
        "",
        None,
        &[
            "<< /Type /StructTreeRoot /K [11 0 R 12 0 R] >>",
            "<< /S /Figure /P 10 0 R /Pg 3 0 R /K 0 >>",
            "<< /S /Figure /P 10 0 R /Pg 3 0 R /K 1 >>",
        ],
    ));
    assert_rect(only(&t, &p, 0, 0), rect(49.0, 49.0, 151.0, 51.0));
    assert_rect(only(&t, &p, 1, 0), rect(198.0, 198.0, 202.0, 302.0));
}

#[test]
fn paint_and_shadings_are_confined_to_the_clip() {
    let (t, p) = read(tagged(
        "q 0 0 60 60 re W n /Figure <</MCID 0>> BDC 0 0 100 100 re f EMC Q\n\
         q 10 10 20 20 re W n /Figure <</MCID 1>> BDC /Sh0 sh EMC Q\n\
         /Figure <</MCID 2>> BDC /Sh0 sh EMC",
        "",
        None,
        &[
            "<< /Type /StructTreeRoot /K [11 0 R 12 0 R 13 0 R] >>",
            "<< /S /Figure /P 10 0 R /Pg 3 0 R /K 0 >>",
            "<< /S /Figure /P 10 0 R /Pg 3 0 R /K 1 >>",
            "<< /S /Figure /P 10 0 R /Pg 3 0 R /K 2 >>",
        ],
    ));
    assert_rect(only(&t, &p, 0, 0), rect(0.0, 0.0, 60.0, 60.0));
    assert_rect(only(&t, &p, 1, 0), rect(10.0, 10.0, 30.0, 30.0));
    // No clip: a shading fills the page's crop box.
    assert_rect(only(&t, &p, 2, 0), rect(0.0, 0.0, 612.0, 792.0));
}

#[test]
fn nested_sequences_paint_for_the_innermost_mcid() {
    let (t, p) = read(tagged(
        "/TD <</MCID 0>> BDC /Span BMC 0 0 5 5 re f EMC \
         /X <</MCID 1>> BDC 300 300 10 10 re f EMC EMC",
        "",
        None,
        &[
            "<< /Type /StructTreeRoot /K [11 0 R 12 0 R] >>",
            "<< /S /TD /P 10 0 R /Pg 3 0 R /K 0 >>",
            "<< /S /Span /P 10 0 R /Pg 3 0 R /K 1 >>",
        ],
    ));
    assert_rect(only(&t, &p, 0, 0), rect(0.0, 0.0, 5.0, 5.0));
    assert_rect(only(&t, &p, 1, 0), rect(300.0, 300.0, 310.0, 310.0));
}

#[test]
fn elements_with_no_bounds_are_named_with_a_reason() {
    let (t, p) = read(tagged(
        "/P <</MCID 0>> BDC EMC /P <</MCID 1>> BDC 0 0 10 10 re f EMC",
        "",
        None,
        &[
            "<< /Type /StructTreeRoot /K [11 0 R 12 0 R 13 0 R 14 0 R 15 0 R 16 0 R] >>",
            "<< /S /P /P 10 0 R /K 5 >>",
            "<< /S /P /P 10 0 R /Pg 3 0 R /K 99 >>",
            "<< /S /P /P 10 0 R /Pg 3 0 R /K 0 >>",
            "<< /S /TD /P 10 0 R >>",
            "<< /S /Figure /P 10 0 R /K << /Type /OBJR /Obj 6 0 R /Pg 3 0 R >> >>",
            "<< /S /TR /P 10 0 R /Pg 3 0 R /K [1 17 0 R] >>",
            "<< /S /TD /P 16 0 R /K 5 >>",
        ],
    ));
    let report: Vec<(usize, NoBoundsReason)> = t
        .elements_without_painted_bounds(&p)
        .iter()
        .map(|u| (u.element, u.reason))
        .collect();
    assert_eq!(
        report,
        [
            (0, NoBoundsReason::PageUnresolved),
            (1, NoBoundsReason::McidNotFound),
            (2, NoBoundsReason::NothingPainted),
            (3, NoBoundsReason::NoContent),
            (4, NoBoundsReason::NothingPainted),
            // Element 5 (TR) has bounds through MCID 1; its child inherits
            // page 0 from it, where MCID 5 is never declared.
            (6, NoBoundsReason::McidNotFound),
        ]
    );
    assert_eq!(NoBoundsReason::McidNotFound.to_string(), "MCID not found");
}

#[test]
fn hostile_trees_and_streams_report_without_panicking() {
    // A cyclic /K, a 3,000-deep element chain, a self-invoking form, an
    // unbalanced stack and non-finite geometry.
    let mut structure: Vec<String> = vec![
        "<< /Type /StructTreeRoot /K [11 0 R 13 0 R] >>".to_owned(),
        "<< /S /Div /P 10 0 R /Pg 3 0 R /K [12 0 R 0] >>".to_owned(),
        "<< /S /Div /P 11 0 R /K [11 0 R] >>".to_owned(),
    ];
    let depth = 3_000;
    for i in 0..depth {
        let n = 13 + i;
        let k = if i + 1 == depth {
            "/K 1".to_owned()
        } else {
            format!("/K {} 0 R", n + 1)
        };
        structure.push(format!("<< /S /Div /P 10 0 R /Pg 3 0 R {k} >>"));
    }
    let refs: Vec<&str> = structure.iter().map(String::as_str).collect();
    let page = "q q q Q Q Q Q Q EMC EMC /P <</MCID 0>> BDC 1e308 1e308 1e308 1e308 re f \
                /Fm1 Do 0 0 m S EMC /P <</MCID 1>> BDC /Fm1 Do EMC";
    let form = "/Fm1 Do 5 5 10 10 re f";
    let (t, p) = read(tagged(page, form, None, &refs));
    assert!(t.elements.len() > depth);
    assert!(p.diagnostics.form_cycles >= 1, "{:?}", p.diagnostics);
    for i in 0..t.elements.len() {
        let _ = t.element_painted_bounds(i, &p);
    }
    let report = t.elements_without_painted_bounds(&p);
    assert!(report.iter().all(|u| u.element < t.elements.len()));
    // MCID 1 paints the form's rect.
    let deepest = t.elements.len() - 1;
    assert_rect(only(&t, &p, deepest, 0), rect(5.0, 5.0, 15.0, 15.0));
}
