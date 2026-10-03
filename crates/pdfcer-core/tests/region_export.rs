//! `pageops::extract_region` on a synthetic page (`Pass 449.0`).
//!
//! The fixture page is 400×400 and the region is `[50 50 250 250]`. Inside:
//! plain text, a text field, a Square annotation, a ce dimension added in an
//! unsaved session, text on a hidden layer and text on a shown layer.
//! Outside: text, a filled rectangle and a second Square annotation. Across
//! the edge: a stroked line and a run of text.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::dimension::{DEFAULT_GROUP_ID, DimensionKind};
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::filters::decode_stream;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_core::pageops::region::{RegionError, RegionExport, RegionReport, extract_region};
use pdfcer_core::text_extract::{ExtractOptions, extract_page};
use pdfcer_core::vector::{AxisConstraint, Point};

const REGION: Rect = Rect {
    llx: 50.0,
    lly: 50.0,
    urx: 250.0,
    ury: 250.0,
};

const PAGE_CONTENT: &str = "\
BT /F1 12 Tf 60 100 Td (INSIDE) Tj ET
BT /F1 12 Tf 300 300 Td (OUTSIDE) Tj ET
BT /F1 12 Tf 200 130 Td (STRADDLING) Tj ET
/OC /L1 BDC BT /F1 12 Tf 60 140 Td (HIDDENLAYER) Tj ET EMC
/OC /L2 BDC BT /F1 12 Tf 60 120 Td (SHOWNLAYER) Tj ET EMC
2 w 20 240 m 380 240 l S
300 20 60 40 re f";

fn stream(dict: &str, data: &str) -> String {
    format!(
        "<< {dict} /Length {} >>\nstream\n{data}\nendstream",
        data.len()
    )
}

fn appearance(w: u32, h: u32, text: &str) -> String {
    stream(
        &format!(
            "/Type /XObject /Subtype /Form /BBox [0 0 {w} {h}] \
             /Resources << /Font << /F1 5 0 R >> >>"
        ),
        &format!("BT /F1 10 Tf 2 5 Td ({text}) Tj ET"),
    )
}

fn fixture_bytes() -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R \
         /AcroForm << /Fields [8 0 R] /DA (/F1 10 Tf 0 g) /DR << /Font << /F1 5 0 R >> >> >> \
         /OCProperties << /OCGs [6 0 R 7 0 R] /D << /Order [6 0 R 7 0 R] /OFF [6 0 R] >> >> >>"
            .to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /TrimBox [0 0 400 400] \
         /Resources << /Font << /F1 5 0 R >> /Properties << /L1 6 0 R /L2 7 0 R >> >> \
         /Contents 4 0 R /Annots [8 0 R 9 0 R 11 0 R] >>"
            .to_string(),
        stream("", PAGE_CONTENT),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_string(),
        "<< /Type /OCG /Name (Hidden) >>".to_string(),
        "<< /Type /OCG /Name (Shown) >>".to_string(),
        "<< /Type /Annot /Subtype /Widget /FT /Tx /T (name) /V (FIELDVAL) \
         /Rect [60 60 160 80] /P 3 0 R /F 4 /DA (/F1 10 Tf 0 g) /AP << /N 10 0 R >> >>"
            .to_string(),
        "<< /Type /Annot /Subtype /Square /Rect [100 160 200 190] /F 4 /AP << /N 12 0 R >> >>"
            .to_string(),
        appearance(100, 20, "FIELDVAL"),
        "<< /Type /Annot /Subtype /Square /Rect [280 100 380 130] /F 4 /AP << /N 13 0 R >> >>"
            .to_string(),
        appearance(100, 30, "ANNOTIN"),
        appearance(100, 30, "ANNOTOUT"),
    ];
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

/// The fixture with a ce dimension inside the region, added in a session
/// that is never saved.
fn session() -> EditSession {
    let mut s = EditSession::new(Document::from_bytes(fixture_bytes()).unwrap());
    let kind = DimensionKind::Linear {
        a: Point::new(80.0, 200.0),
        b: Point::new(200.0, 200.0),
        constraint: AxisConstraint::Horizontal,
        offset: 0.0,
        text_along: 0.0,
        extension_gap: [None; 2],
    };
    s.add_dimension(0, DEFAULT_GROUP_ID, kind).unwrap();
    s
}

fn export(state: &RegionExport) -> (Document, RegionReport) {
    let s = session();
    let (bytes, report) = extract_region(&s.view(), 0, REGION, state).unwrap();
    (Document::from_bytes(bytes).unwrap(), report)
}

fn text(doc: &Document) -> String {
    let pages = page_tree::pages(doc).unwrap();
    extract_page(doc, &pages[0], 0, &ExtractOptions::default())
        .unwrap()
        .runs
        .iter()
        .map(|r| r.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Every stream in the file, decoded, concatenated.
fn all_stream_bytes(doc: &Document) -> Vec<u8> {
    let mut out = Vec::new();
    for n in 1..doc.next_object_number().unwrap() {
        if let Some(Object::Stream(s)) = doc.value(ObjId::new(n, 0)) {
            let raw = s.data_span.slice(doc.bytes()).unwrap_or_default();
            out.extend(decode_stream(&s.dict, raw).unwrap_or_default());
        }
    }
    out
}

fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

fn page_dict(doc: &Document) -> pdfcer_core::object::Dict {
    let id = page_tree::pages(doc).unwrap()[0].id;
    doc.value(id).and_then(Object::as_dict).cloned().unwrap()
}

#[test]
fn every_box_is_the_region() {
    let (doc, report) = export(&RegionExport::new());
    let page = &page_tree::pages(&doc).unwrap()[0];
    assert_eq!(page.media_box, REGION);
    assert_eq!(page.crop_box, REGION);
    assert_eq!(page.trim_box, REGION);
    assert_eq!(report.boxes_clamped, 1);
    assert_eq!(report.rect, REGION);
}

#[test]
fn outside_content_is_gone_from_every_stream() {
    let (doc, report) = export(&RegionExport::new());
    let streams = all_stream_bytes(&doc);
    for gone in ["OUTSIDE", "ANNOTOUT", "HIDDENLAYER", "STRADDLING"] {
        assert!(!contains(&streams, gone), "{gone} survived in a stream");
    }
    assert!(report.paths_dropped >= 1, "{report:?}");
    assert!(report.forms_dropped >= 1, "{report:?}");
    assert!(!report.has_residuals(), "{report:?}");
}

#[test]
fn inside_content_is_kept() {
    let (doc, _) = export(&RegionExport::new());
    let t = text(&doc);
    for kept in ["INSIDE", "SHOWNLAYER", "FIELDVAL", "ANNOTIN"] {
        assert!(t.contains(kept), "{kept} missing from {t:?}");
    }
}

#[test]
fn straddling_text_loses_the_glyphs_that_cross_the_edge() {
    let (doc, report) = export(&RegionExport::new());
    let t = text(&doc);
    assert!(t.contains("STRAD"), "{t:?}");
    assert!(!t.contains("STRADDLING"), "{t:?}");
    assert!(report.glyphs_removed >= 3, "{report:?}");
}

#[test]
fn straddling_path_is_cut_at_the_edge() {
    let (_, report) = export(&RegionExport::new());
    assert!(report.paths_cut >= 1, "{report:?}");
    assert_eq!(report.paths_uncut, 0, "{report:?}");
}

#[test]
fn inside_text_keeps_its_position() {
    let s = session();
    let before = glyph_origin(&Document::from_bytes(fixture_bytes()).unwrap(), "INSIDE");
    let (bytes, _) = extract_region(&s.view(), 0, REGION, &RegionExport::new()).unwrap();
    let after = glyph_origin(&Document::from_bytes(bytes).unwrap(), "INSIDE");
    assert!((before.0 - after.0).abs() < 1e-3 && (before.1 - after.1).abs() < 1e-3);
}

fn glyph_origin(doc: &Document, run: &str) -> (f32, f32) {
    let pages = page_tree::pages(doc).unwrap();
    let text = extract_page(doc, &pages[0], 0, &ExtractOptions::default()).unwrap();
    let r = text.runs.iter().find(|r| r.text.contains(run)).unwrap();
    (r.glyphs[0].x, r.glyphs[0].y)
}

#[test]
fn annotations_on_flattens_fields_annotations_and_ce_dimensions() {
    let (doc, report) = export(&RegionExport::new());
    assert_eq!(report.fields_flattened, 1, "{report:?}");
    assert_eq!(report.widgets_flattened, 1, "{report:?}");
    assert_eq!(report.ce_dimensions_flattened, 1, "{report:?}");
    assert_eq!(report.annotations_flattened, 3, "{report:?}");
    let page = page_dict(&doc);
    assert!(page.get(b"Annots").is_none());
    assert!(doc.catalog().unwrap().get(b"AcroForm").is_none());
}

#[test]
fn annotations_off_removes_them_all() {
    let (doc, report) = export(&RegionExport::new().with_annotations(false));
    assert_eq!(report.annotations_flattened, 0);
    assert_eq!(report.fields_flattened, 0);
    assert_eq!(report.annotations_removed, 4, "{report:?}");
    assert!(page_dict(&doc).get(b"Annots").is_none());
    let t = text(&doc);
    assert!(!t.contains("FIELDVAL") && !t.contains("ANNOTIN"), "{t:?}");
    assert!(t.contains("INSIDE"), "{t:?}");
}

#[test]
fn default_layer_state_removes_hidden_layer_content() {
    let (doc, report) = export(&RegionExport::new());
    assert_eq!(report.layers_hidden, 1, "{report:?}");
    assert!(report.layer_content_removed >= 1, "{report:?}");
    let t = text(&doc);
    assert!(
        !t.contains("HIDDENLAYER") && t.contains("SHOWNLAYER"),
        "{t:?}"
    );
    assert!(doc_has_no_layers(&doc));
}

#[test]
fn layer_override_replaces_the_default_state() {
    let state = RegionExport::new().with_hidden_layers([ObjId::new(7, 0)]);
    let (doc, report) = export(&state);
    assert_eq!(report.layers_hidden, 1, "{report:?}");
    let t = text(&doc);
    assert!(
        t.contains("HIDDENLAYER") && !t.contains("SHOWNLAYER"),
        "{t:?}"
    );
}

fn doc_has_no_layers(doc: &Document) -> bool {
    !contains(&all_stream_bytes(doc), "/OC ")
}

#[test]
fn an_empty_or_infinite_rect_is_refused() {
    let s = session();
    for bad in [
        Rect::from_corners(10.0, 10.0, 10.0, 200.0),
        Rect::from_corners(0.0, 0.0, f64::INFINITY, 10.0),
    ] {
        let err = extract_region(&s.view(), 0, bad, &RegionExport::new()).unwrap_err();
        assert!(matches!(err, RegionError::InvalidRect), "{err:?}");
    }
}

#[test]
fn a_page_past_the_end_is_refused() {
    let s = session();
    let err = extract_region(&s.view(), 3, REGION, &RegionExport::new()).unwrap_err();
    assert!(matches!(err, RegionError::PageOp(_)), "{err:?}");
}
