//! `scale_pages` places a page's fill patterns with its content
//! (ISO 32000-2 §8.7.3.1: a pattern maps to the page's default space).

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::pageops::{ScaleReport, ScaleRequest};
use pdfcer_core::writer::SaveOptions;

const SHADING_MATRIX: [f64; 6] = [2.0, 0.0, 0.0, 2.0, 100.0, 100.0];

fn build_pdf(bodies: &[String]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
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

/// Two letter pages inheriting one `/Resources` from their `/Pages` node:
/// a shading pattern `/P0` (obj 5, `SHADING_MATRIX`), a tiling pattern
/// `/T0` (obj 6, no `/Matrix`) and a shading pattern `/Bad` whose
/// `/Matrix` is five numbers (obj 7).
fn two_pages() -> Vec<u8> {
    let content = "/Pattern cs /P0 scn 0 0 612 792 re f";
    let tile = "0 0 10 10 re f";
    let shading = "/ShadingType 2 /ColorSpace /DeviceGray /Coords [0 0 1 0] /Function \
                   << /FunctionType 2 /Domain [0 1] /N 1 >>";
    build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 612 792] \
         /Resources << /Pattern << /P0 5 0 R /T0 6 0 R /Bad 7 0 R >> >> >>"
            .into(),
        "<< /Type /Page /Parent 2 0 R /Contents 8 0 R >>".into(),
        "<< /Type /Page /Parent 2 0 R /Contents 8 0 R >>".into(),
        format!("<< /PatternType 2 /Matrix [2 0 0 2 100 100] /Shading << {shading} >> >>"),
        format!(
            "<< /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] /XStep 10 \
             /YStep 10 /Resources << >> /Length {} >>\nstream\n{tile}\nendstream",
            tile.len()
        ),
        format!("<< /PatternType 2 /Matrix [1 0 0 1 0] /Shading << {shading} >> >>"),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
    ])
}

/// Page 0 scaled to A4 (so the placement has a non-zero offset).
fn scaled() -> (EditSession, ScaleReport) {
    let mut s = EditSession::new(Document::from_bytes(two_pages()).unwrap());
    let report = s
        .scale_pages(&[0], &ScaleRequest::new(595.0, 842.0))
        .unwrap();
    // The saved file reads back.
    let (bytes, _) = s.to_incremental_bytes(&SaveOptions::identity()).unwrap();
    let reread = EditSession::new(Document::from_bytes(bytes).unwrap());
    assert_eq!(reread.pages().unwrap().len(), 2);
    (s, report)
}

/// The object page `page`'s resources name as pattern `name`, resolved.
fn pattern_of(s: &EditSession, page: ObjId, name: &[u8]) -> (Object, Object) {
    let g = s.graph();
    let pd = g.resolved(page).as_dict().unwrap().clone();
    let res = match pd.get(b"Resources") {
        Some(r) => g.resolve(r).clone(),
        None => {
            let parent = g.resolved(ObjId::new(2, 0)).as_dict().unwrap().clone();
            g.resolve(parent.get(b"Resources").unwrap()).clone()
        }
    };
    let pats = g
        .resolve(res.as_dict().unwrap().get(b"Pattern").unwrap())
        .clone();
    let held = pats.as_dict().unwrap().get(name).unwrap().clone();
    (held.clone(), g.resolve(&held).clone())
}

fn matrix(pattern: &Object) -> [f64; 6] {
    let d = match pattern {
        Object::Stream(st) => &st.dict,
        other => other.as_dict().unwrap(),
    };
    let m: Vec<f64> = d
        .get(b"Matrix")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_number().unwrap())
        .collect();
    m.try_into().unwrap()
}

fn apply(m: [f64; 6], (u, v): (f64, f64)) -> (f64, f64) {
    (u * m[0] + v * m[2] + m[4], u * m[1] + v * m[3] + m[5])
}

#[test]
fn a_page_pattern_follows_the_content_it_fills() {
    let (s, report) = scaled();
    let p = report.pages[0].placement;
    assert!(p.offset_y != 0.0 || p.offset_x != 0.0, "{p:?}");
    assert_eq!(report.pages[0].patterns, 2, "P0 and T0");

    let (held, placed) = pattern_of(&s, ObjId::new(3, 0), b"P0");
    assert_ne!(
        held,
        Object::Reference(ObjId::new(5, 0)),
        "a copy, not the original"
    );
    for q in [(0.0, 0.0), (1.0, 0.0), (3.0, -7.5)] {
        let (x, y) = apply(SHADING_MATRIX, q);
        let (px, py) = apply(matrix(&placed), q);
        assert!(
            (px - p.x(x)).abs() < 1e-9 && (py - p.y(y)).abs() < 1e-9,
            "{q:?}"
        );
    }

    // An absent /Matrix is identity, so the tiling copy carries the
    // placement itself, and keeps its tile.
    let (_, tile) = pattern_of(&s, ObjId::new(3, 0), b"T0");
    assert_eq!(
        matrix(&tile),
        [p.scale, 0.0, 0.0, p.scale, p.offset_x, p.offset_y]
    );
    let Object::Stream(st) = &tile else {
        panic!("{tile:?}")
    };
    assert_eq!(s.graph().stream_raw(st).unwrap(), b"0 0 10 10 re f");

    // The copy survives both saves.
    for bytes in [
        s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0,
        s.to_full_bytes(&SaveOptions::identity()).unwrap().0,
    ] {
        let reread = EditSession::new(Document::from_bytes(bytes).unwrap());
        let (_, tile) = pattern_of(&reread, ObjId::new(3, 0), b"T0");
        let Object::Stream(st) = &tile else {
            panic!("{tile:?}")
        };
        assert_eq!(reread.graph().stream_raw(st).unwrap(), b"0 0 10 10 re f");
        assert_eq!(matrix(&tile)[0], p.scale);
    }
}

#[test]
fn a_page_sharing_the_pattern_and_not_scaled_keeps_the_original() {
    let (s, _) = scaled();
    let (held, original) = pattern_of(&s, ObjId::new(4, 0), b"P0");
    assert_eq!(held, Object::Reference(ObjId::new(5, 0)));
    assert_eq!(matrix(&original), SHADING_MATRIX);
    assert!(!s.dirty_set().contains(ObjId::new(5, 0)));
    assert!(
        !s.dirty_set().contains(ObjId::new(2, 0)),
        "the shared resources are untouched"
    );
}

#[test]
fn an_unreadable_pattern_matrix_is_left_unchanged() {
    let (s, report) = scaled();
    assert_eq!(report.patterns_unchanged, 1);
    let (held, _) = pattern_of(&s, ObjId::new(3, 0), b"Bad");
    assert_eq!(held, Object::Reference(ObjId::new(7, 0)));
    assert!(!s.dirty_set().contains(ObjId::new(7, 0)));
}
