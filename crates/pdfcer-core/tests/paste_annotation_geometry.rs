//! A pasted annotation's geometry keys move with its `/Rect` (`Pass 568.0`,
//! pdfcer-gui G186): a pasted `/Link` whose `/QuadPoints` stayed at the
//! source was clickable only where the original already was.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::vector::Matrix;

const QUADS: &str = "124 152.5 316 152.5 124 107.5 316 107.5";

/// One page holding a `/Link`. With `by_reference` its `/QuadPoints` is an
/// indirect array (object 6), which a paste must rewrite where it is
/// written rather than leave pointing at an unmoved copy.
fn doc_with_link(by_reference: bool) -> Document {
    let quads = if by_reference {
        "6 0 R".to_owned()
    } else {
        format!("[{QUADS}]")
    };
    let mut objects = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned()),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
             /Annots [5 0 R] >>"
                .to_owned(),
        ),
        (4, "<< /Length 0 >>\nstream\n\nendstream".to_owned()),
        (
            5,
            format!(
                "<< /Type /Annot /Subtype /Link /Rect [100 100 340 160] /Border [0 0 0] \
                 /QuadPoints {quads} /A << /S /URI /URI (https://example.invalid/) >> >>"
            ),
        ),
    ];
    if by_reference {
        objects.push((6, format!("[{QUADS}]")));
    }
    let mut out = String::from("%PDF-1.7\n");
    let mut offsets = Vec::new();
    for (num, body) in &objects {
        offsets.push(out.len());
        out.push_str(&format!("{num} 0 obj\n{body}\nendobj\n"));
    }
    let xref_at = out.len();
    out.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for off in offsets {
        out.push_str(&format!("{off:010} 00000 n \n"));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
        objects.len() + 1
    ));
    Document::from_bytes(out.into_bytes()).expect("fixture must load")
}

fn numbers(o: &Object) -> Vec<f64> {
    o.as_array()
        .expect("an array")
        .iter()
        .map(|n| n.as_number().expect("a number"))
        .collect()
}

/// Paste the page's one link at `(dx, dy)`; returns the pasted link's
/// resolved `/QuadPoints` and the raw value held under the key.
fn paste_link(by_reference: bool, dx: f64, dy: f64) -> (EditSession, Vec<f64>, Object) {
    let mut s = EditSession::new(doc_with_link(by_reference));
    let clip = s.copy_annotations(0, &[0]).expect("copy");
    let outcome = s
        .paste_objects(0, &clip, Matrix::translate(dx, dy))
        .expect("paste");
    assert_eq!(outcome.annotations_pasted, 1);
    let page = s.page_slots().expect("slots")[0].id;
    let placed = pdfcer_core::annot::page_annotations(&s.graph(), page);
    assert_eq!(placed.len(), 2, "the source and the paste");
    let id: ObjId = placed.last().and_then(|a| a.id).expect("an id");
    let graph = s.graph();
    let dict = graph.resolved(id).as_dict().cloned().expect("a dict");
    let held = dict.get(b"QuadPoints").cloned().expect("/QuadPoints");
    let quads = numbers(graph.resolve(&held));
    (s, quads, held)
}

fn expected(dx: f64, dy: f64) -> Vec<f64> {
    QUADS
        .split(' ')
        .map(|n| n.parse::<f64>().unwrap())
        .enumerate()
        .map(|(i, v)| if i % 2 == 0 { v + dx } else { v + dy })
        .collect()
}

#[test]
fn a_pasted_links_quads_move_with_its_rect() {
    let (_, quads, _) = paste_link(false, -20.0, 69.82);
    let want = expected(-20.0, 69.82);
    for (got, want) in quads.iter().zip(&want) {
        assert!((got - want).abs() < 1e-3, "{quads:?} vs {want:?}");
    }
}

/// An indirect `/QuadPoints` is rewritten in the object the paste writes:
/// still a reference, now to the moved array, and the source's untouched.
#[test]
fn a_referenced_quads_array_is_moved_where_it_is_written() {
    let (s, quads, held) = paste_link(true, 30.0, -40.0);
    let want = expected(30.0, -40.0);
    for (got, want) in quads.iter().zip(&want) {
        assert!((got - want).abs() < 1e-3, "{quads:?} vs {want:?}");
    }
    let r = held.as_reference().expect("still held by reference");
    assert_ne!(
        r,
        ObjId::new(6, 0),
        "the paste's own copy, not the source's"
    );
    let graph = s.graph();
    let source = numbers(graph.resolved(ObjId::new(6, 0)));
    assert_eq!(source, expected(0.0, 0.0), "the source link did not move");
}

/// The paste is one command: undo removes it and leaves the source as it was.
#[test]
fn undoing_the_paste_leaves_the_source_link_alone() {
    let (mut s, _, _) = paste_link(false, 10.0, 10.0);
    assert!(s.undo().is_some());
    let page = s.page_slots().expect("slots")[0].id;
    let graph = s.graph();
    let placed = pdfcer_core::annot::page_annotations(&graph, page);
    assert_eq!(placed.len(), 1);
    let dict = graph
        .resolved(placed[0].id.expect("id"))
        .as_dict()
        .cloned()
        .expect("dict");
    assert_eq!(
        numbers(dict.get(b"QuadPoints").expect("quads")),
        expected(0.0, 0.0)
    );
}
