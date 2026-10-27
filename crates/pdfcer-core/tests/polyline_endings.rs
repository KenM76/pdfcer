//! Line endings (`/LE`) and interior colour (`/IC`) on `/PolyLine`
//! (ISO 32000-1 §12.5.6.9 Table 178): authored, restyled and reshaped, with
//! every assertion made against re-parsed saved bytes. Plus the dashed
//! `/Line` shaft, whose heads must stay solid.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;

use pdfcer_core::annot_author::{BorderDash, Color, LineEnding, MarkupSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, MarkupOptions, MarkupStyle, StyleEdit};
use pdfcer_core::object::{Dict, ObjId, Object};
use pdfcer_core::writer::SaveOptions;

const VERTS: [(f64, f64); 3] = [(100.0, 100.0), (200.0, 100.0), (200.0, 200.0)];

fn session() -> EditSession {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/annot/demo-annotated.pdf");
    EditSession::new(Document::load(&p).expect("load fixture"))
}

fn reload(s: &EditSession) -> Document {
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("incremental save");
    Document::from_bytes(bytes).expect("re-parse")
}

fn dict_of(doc: &Document, id: ObjId) -> Dict {
    match &doc.get(id).expect("object present").value {
        Object::Dict(d) => d.clone(),
        other => panic!("object {id} is not a dictionary: {other:?}"),
    }
}

fn ap_bytes(doc: &Document, d: &Dict) -> Vec<u8> {
    let Some(Object::Dict(ap)) = d.get(b"AP") else {
        panic!("no /AP");
    };
    let n = ap.get(b"N").and_then(Object::as_reference).expect("/N ref");
    match &doc.get(n).expect("stream").value {
        Object::Stream(st) => st.data_span.slice(doc.bytes()).expect("raw").to_vec(),
        other => panic!("/N is not a stream: {other:?}"),
    }
}

fn names(d: &Dict, key: &[u8]) -> Option<Vec<Vec<u8>>> {
    match d.get(key)? {
        Object::Array(a) => Some(
            a.iter()
                .map(|o| match o {
                    Object::Name(n) => n.as_bytes().to_vec(),
                    other => panic!("not a name: {other:?}"),
                })
                .collect(),
        ),
        other => panic!("not an array: {other:?}"),
    }
}

fn nums(d: &Dict, key: &[u8]) -> Vec<f64> {
    match d.get(key) {
        Some(Object::Array(a)) => a.iter().map(|o| o.as_number().expect("num")).collect(),
        other => panic!("not a numeric array: {other:?}"),
    }
}

/// Every `x y m` / `x y l` point in a content stream, in order.
fn path_points(content: &[u8]) -> Vec<(f64, f64)> {
    let text = String::from_utf8_lossy(content);
    let toks: Vec<&str> = text.split_whitespace().collect();
    let mut out = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        if (*t == "m" || *t == "l")
            && i >= 2
            && let (Ok(x), Ok(y)) = (toks[i - 2].parse(), toks[i - 1].parse())
        {
            out.push((x, y));
        }
    }
    out
}

fn has_point(points: &[(f64, f64)], want: (f64, f64)) -> bool {
    points
        .iter()
        .any(|p| (p.0 - want.0).abs() < 1e-3 && (p.1 - want.1).abs() < 1e-3)
}

fn polyline(endings: (LineEnding, LineEnding), interior: Option<Color>) -> MarkupSpec {
    MarkupSpec::PolyLine {
        vertices: VERTS.to_vec(),
        color: Color::Gray(0.0),
        width: 2.0,
        endings,
        interior,
    }
}

// Width 2 => arrow length max(8, 8) = 8, half-width 8·tan(22.5°).
const HALF: f64 = 8.0 * 0.414_213_562_373_095;

#[test]
fn heads_follow_the_first_and_last_segments_and_le_ic_are_written() {
    let mut s = session();
    let red = Color::Rgb(1.0, 0.0, 0.0);
    let id = s
        .add_markup(
            0,
            &polyline((LineEnding::ClosedArrow, LineEnding::OpenArrow), Some(red)),
        )
        .unwrap();
    let doc = reload(&s);
    let d = dict_of(&doc, id);
    assert_eq!(
        names(&d, b"LE").unwrap(),
        vec![b"ClosedArrow".to_vec(), b"OpenArrow".to_vec()]
    );
    assert_eq!(nums(&d, b"IC"), vec![1.0, 0.0, 0.0]);

    let pts = path_points(&ap_bytes(&doc, &d));
    // Start: tip (100,100), shaft from (200,100) => barbs back along +x.
    assert!(has_point(&pts, (108.0, 100.0 - HALF)), "{pts:?}");
    assert!(has_point(&pts, (108.0, 100.0 + HALF)), "{pts:?}");
    // End: tip (200,200), shaft from (200,100) => barbs back along -y.
    assert!(has_point(&pts, (200.0 - HALF, 192.0)), "{pts:?}");
    assert!(has_point(&pts, (200.0 + HALF, 192.0)), "{pts:?}");

    // The closed head is filled (`B`), the shaft is not.
    let ap = String::from_utf8_lossy(&ap_bytes(&doc, &d)).to_string();
    assert!(ap.split_whitespace().any(|t| t == "B"), "{ap}");

    // /Rect encloses the barbs.
    let r = nums(&d, b"Rect");
    assert!(
        r[0] <= 100.0 - 8.0 + 1e-6 && r[3] >= 200.0 + 8.0 - 1e-6,
        "{r:?}"
    );
}

#[test]
fn no_endings_writes_no_le_and_draws_no_heads() {
    let mut s = session();
    let id = s
        .add_markup(0, &polyline((LineEnding::None, LineEnding::None), None))
        .unwrap();
    let doc = reload(&s);
    let d = dict_of(&doc, id);
    assert!(d.get(b"LE").is_none());
    assert!(d.get(b"IC").is_none());
    let pts = path_points(&ap_bytes(&doc, &d));
    assert_eq!(pts.len(), 3, "the three vertices and nothing else: {pts:?}");
}

#[test]
fn restyle_sets_and_clears_polyline_endings() {
    let mut s = session();
    let id = s
        .add_markup(0, &polyline((LineEnding::None, LineEnding::None), None))
        .unwrap();
    s.set_markup_style(
        id,
        &MarkupStyle {
            endings: Some(StyleEdit::Set((
                LineEnding::OpenArrow,
                LineEnding::ClosedArrow,
            ))),
            ..MarkupStyle::default()
        },
    )
    .expect("a polyline takes endings");
    let doc = reload(&s);
    let d = dict_of(&doc, id);
    assert_eq!(
        names(&d, b"LE").unwrap(),
        vec![b"OpenArrow".to_vec(), b"ClosedArrow".to_vec()]
    );
    let pts = path_points(&ap_bytes(&doc, &d));
    assert!(has_point(&pts, (200.0 - HALF, 192.0)), "{pts:?}");

    s.set_markup_style(
        id,
        &MarkupStyle {
            endings: Some(StyleEdit::Clear),
            ..MarkupStyle::default()
        },
    )
    .unwrap();
    let doc = reload(&s);
    let d = dict_of(&doc, id);
    assert!(d.get(b"LE").is_none());
    assert_eq!(path_points(&ap_bytes(&doc, &d)).len(), 3);
}

#[test]
fn reshape_rebakes_the_heads_at_the_moved_vertex() {
    let mut s = session();
    let id = s
        .add_markup(
            0,
            &polyline((LineEnding::None, LineEnding::OpenArrow), None),
        )
        .unwrap();
    // Last vertex (200,200) -> (300,100): last segment now runs along +x.
    s.move_annotation_vertex(id, 2, 100.0, -100.0).unwrap();
    let doc = reload(&s);
    let d = dict_of(&doc, id);
    assert_eq!(
        names(&d, b"LE").unwrap(),
        vec![b"None".to_vec(), b"OpenArrow".to_vec()]
    );
    let pts = path_points(&ap_bytes(&doc, &d));
    assert!(has_point(&pts, (292.0, 100.0 + HALF)), "{pts:?}");
    assert!(has_point(&pts, (292.0, 100.0 - HALF)), "{pts:?}");
    assert!(
        !has_point(&pts, (200.0 - HALF, 192.0)),
        "old head gone: {pts:?}"
    );
}

/// The dash operator's operands as `(array_is_empty)`, in stream order.
fn dash_ops(content: &[u8]) -> Vec<bool> {
    let text = String::from_utf8_lossy(content);
    let mut out = Vec::new();
    let mut rest = text.as_ref();
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find(']') else {
            break;
        };
        let inner = rest[open + 1..open + close].trim();
        let after = &rest[open + close + 1..];
        let mut t = after.split_whitespace();
        if let (Some(_phase), Some("d")) = (t.next(), t.next()) {
            out.push(inner.is_empty());
        }
        rest = after;
    }
    out
}

#[test]
fn a_dashed_shaft_gets_solid_heads_on_line_and_polyline() {
    let opts = MarkupOptions {
        dash: Some(BorderDash::new(vec![4.0, 2.0]).unwrap()),
        ..MarkupOptions::default()
    };
    let line = MarkupSpec::Line {
        start: (100.0, 100.0),
        end: (200.0, 150.0),
        color: Color::Gray(0.0),
        width: 1.0,
        endings: (LineEnding::OpenArrow, LineEnding::OpenArrow),
        interior: None,
    };
    let poly = polyline((LineEnding::OpenArrow, LineEnding::None), None);
    for spec in [line, poly] {
        let mut s = session();
        let id = s.add_markup_with(0, &spec, &opts).unwrap();
        let doc = reload(&s);
        let ops = dash_ops(&ap_bytes(&doc, &dict_of(&doc, id)));
        assert_eq!(ops, vec![false, true], "dashed shaft, then solid heads");
    }
}
