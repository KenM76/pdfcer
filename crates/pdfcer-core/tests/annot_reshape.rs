//! `Pass 255.0` — reshaping a markup annotation one vertex at a time.
//!
//! `pdfcer-gui` request 2026-09-05 (*"I also can't edit or delete nodes of a
//! markup shape once it is drawn"*). Three things are under test, and each
//! is a claim the code makes that a wrong implementation would still pass a
//! weaker test for:
//!
//! 1. **The read model.** `Annotation::vertices` / `line` / `ink_list` are
//!    populated from `/Vertices`, `/L`, `/InkList` — the prerequisite the
//!    request put first, because a shell cannot draw an anchor it cannot
//!    read.
//! 2. **The per-subtype matrix, by name.** Polygon/PolyLine take all three
//!    operations and refuse below their floor; Line moves only; Ink,
//!    Square/Circle and text markup refuse everything — each with
//!    [`EditError::GeometryNotReshapable`] and a stated reason, never silence.
//! 3. **One bake, not two.** A reshaped cloud's appearance stream is
//!    byte-identical to the stream `add_markup` would author for the same
//!    vertices. If a second cloud function ever crept in, edit-in-place and
//!    delete-and-redraw would draw different scallops for the same shape,
//!    and this is the test that would say so.
//!
//! Plus the two lock gates as two tests, because the spec makes them two
//! flags and treating them as one is a bug in either direction.
//!
//! Every assertion is made against **re-parsed saved bytes**, not the
//! in-memory session — the shell's next read of the file is what matters.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use pdfcer_core::annot::page_annotations;
use pdfcer_core::annot_author::{Color, LineEnding, MarkupSpec, TextMarkupKind};
use pdfcer_core::dimension::{DEFAULT_GROUP_ID, DimensionKind};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{
    AppearanceWrite, CommandKind, EditError, EditSession, MarkupNote, VertexEdit, VertexEditKind,
};
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::vector::{AxisConstraint, Point};
use pdfcer_core::writer::SaveOptions;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn session(rel: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(rel)).expect("load fixture"))
}

/// Save incrementally and re-parse — every assertion is about the bytes.
fn reload(s: &EditSession) -> Document {
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("incremental save");
    Document::from_bytes(bytes).expect("re-parse")
}

fn dict_of(doc: &Document, id: ObjId) -> pdfcer_core::object::Dict {
    match &doc.get(id).expect("object present").value {
        Object::Dict(d) => d.clone(),
        other => panic!("object {id} is not a dictionary: {other:?}"),
    }
}

/// A flat numeric array from a re-parsed dictionary.
fn nums(doc: &Document, d: &pdfcer_core::object::Dict, key: &[u8]) -> Vec<f64> {
    match d.get(key).map(|o| doc.view().resolve(o).clone()) {
        Some(Object::Array(items)) => items
            .iter()
            .map(|o| doc.view().resolve(o).as_number().expect("numeric"))
            .collect(),
        other => panic!(
            "key {} not a numeric array: {other:?}",
            String::from_utf8_lossy(key)
        ),
    }
}

fn pairs(doc: &Document, d: &pdfcer_core::object::Dict, key: &[u8]) -> Vec<(f64, f64)> {
    nums(doc, d, key)
        .chunks_exact(2)
        .map(|c| (c[0], c[1]))
        .collect()
}

/// The raw bytes of the annotation's `/AP` `/N` stream after a save.
fn ap_bytes(doc: &Document, d: &pdfcer_core::object::Dict) -> Vec<u8> {
    let Some(Object::Dict(ap)) = d.get(b"AP") else {
        panic!("no /AP");
    };
    let n = ap.get(b"N").and_then(Object::as_reference).expect("/N ref");
    match &doc.get(n).expect("stream present").value {
        Object::Stream(st) => st.data_span.slice(doc.bytes()).expect("raw bytes").to_vec(),
        other => panic!("/N is not a stream: {other:?}"),
    }
}

/// A session with one authored markup of the given spec on page 1.
fn with_markup(spec: &MarkupSpec) -> (EditSession, ObjId) {
    let mut s = session("annot/demo-annotated.pdf");
    let id = s.add_markup(0, spec).expect("author the markup");
    (s, id)
}

fn polygon(vertices: Vec<(f64, f64)>) -> MarkupSpec {
    MarkupSpec::Polygon {
        vertices,
        border: Some(Color::Rgb(1.0, 0.0, 0.0)),
        interior: None,
        width: 2.0,
    }
}

fn polyline(vertices: Vec<(f64, f64)>) -> MarkupSpec {
    MarkupSpec::PolyLine {
        vertices,
        color: Color::Gray(0.0),
        width: 1.5,
    }
}

fn cloud(vertices: Vec<(f64, f64)>) -> MarkupSpec {
    MarkupSpec::Cloud {
        vertices,
        border: Some(Color::Rgb(0.0, 0.0, 1.0)),
        interior: None,
        width: 1.0,
        intensity: 1.0,
    }
}

fn line() -> MarkupSpec {
    MarkupSpec::Line {
        start: (50.0, 50.0),
        end: (250.0, 120.0),
        color: Color::Gray(0.0),
        width: 1.0,
        endings: (LineEnding::None, LineEnding::OpenArrow),
    }
}

fn ink() -> MarkupSpec {
    MarkupSpec::Ink {
        strokes: vec![
            vec![(10.0, 10.0), (20.0, 30.0), (25.0, 35.0)],
            vec![(40.0, 40.0), (50.0, 60.0)],
        ],
        color: Color::Gray(0.0),
        width: 2.0,
    }
}

const TRI: [(f64, f64); 3] = [(100.0, 100.0), (200.0, 100.0), (150.0, 180.0)];
const QUAD: [(f64, f64); 4] = [
    (100.0, 100.0),
    (200.0, 100.0),
    (200.0, 200.0),
    (100.0, 200.0),
];

/// A one-page PDF with a hand-written `/Polygon` carrying `/F flags` — the
/// only way to get a Locked / LockedContents annotation without a verb for
/// setting flags, and better than patching bytes.
fn pdf_with_flagged_polygon(flags: u32) -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Resources << >> /Annots [4 0 R] >>"
            .to_owned(),
        format!(
            "<< /Type /Annot /Subtype /Polygon /Rect [99 99 201 201] \
             /Vertices [100 100 200 100 200 200 100 200] /C [1 0 0] /F {flags} >>"
        ),
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

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

// ---------------------------------------------------------------------------
// 1. The read model — the prerequisite
// ---------------------------------------------------------------------------

#[test]
fn the_read_model_exposes_vertices_line_and_ink_list() {
    let (s, id) = with_markup(&polygon(QUAD.to_vec()));
    let doc = reload(&s);
    let slots = s.page_slots().unwrap();
    let annots = page_annotations(&doc.view(), slots[0].id);
    let a = annots
        .iter()
        .find(|a| a.id == Some(id))
        .expect("polygon listed");
    assert_eq!(a.vertices.as_deref(), Some(&QUAD[..]));
    assert!(a.line.is_none(), "a polygon has no /L");
    assert!(a.ink_list.is_none(), "a polygon has no /InkList");

    let (s, id) = with_markup(&line());
    let doc = reload(&s);
    let annots = page_annotations(&doc.view(), slots[0].id);
    let a = annots
        .iter()
        .find(|a| a.id == Some(id))
        .expect("line listed");
    assert_eq!(a.line, Some([(50.0, 50.0), (250.0, 120.0)]));
    assert!(a.vertices.is_none());

    let (s, id) = with_markup(&ink());
    let doc = reload(&s);
    let annots = page_annotations(&doc.view(), slots[0].id);
    let a = annots
        .iter()
        .find(|a| a.id == Some(id))
        .expect("ink listed");
    let strokes = a.ink_list.as_ref().expect("ink list read");
    assert_eq!(strokes.len(), 2);
    assert_eq!(strokes[0], vec![(10.0, 10.0), (20.0, 30.0), (25.0, 35.0)]);
    assert_eq!(strokes[1], vec![(40.0, 40.0), (50.0, 60.0)]);

    // A rectangle-based shape reads as None on all three — not as an empty
    // list, which would look like "a polygon with no vertices".
    let (s, id) = with_markup(&MarkupSpec::Square {
        rect: pdfcer_core::page_tree::Rect {
            llx: 10.0,
            lly: 10.0,
            urx: 50.0,
            ury: 50.0,
        },
        border: Some(Color::Gray(0.0)),
        interior: None,
        border_width: 1.0,
        border_effect: None,
    });
    let doc = reload(&s);
    let annots = page_annotations(&doc.view(), slots[0].id);
    let a = annots
        .iter()
        .find(|a| a.id == Some(id))
        .expect("square listed");
    assert!(a.vertices.is_none() && a.line.is_none() && a.ink_list.is_none());
}

// ---------------------------------------------------------------------------
// 2. Polygon / PolyLine — move, insert, remove, floor
// ---------------------------------------------------------------------------

#[test]
fn moving_a_polygon_vertex_rewrites_vertices_rect_and_appearance_in_place() {
    let (mut s, id) = with_markup(&polygon(QUAD.to_vec()));
    // A note, so there is a key the verb does not own to prove survives.
    s.set_markup_note(
        id,
        &MarkupNote::new("keep me").by("Ken").at("D:20260905120000Z"),
    )
    .unwrap();
    let before = reload(&s);
    let before_d = dict_of(&before, id);
    let ap_before = before_d.get(b"AP").cloned();

    let r = s.move_annotation_vertex(id, 2, 30.0, -10.0).expect("move");
    assert_eq!(r.edit, VertexEditKind::Moved);
    assert_eq!((r.vertices_before, r.vertices_after), (4, 4));
    assert_eq!(r.subtype, "Polygon");
    assert!(!r.mod_date_written, "the wrapper does not touch /M");
    assert!(!r.measure_not_recomputed, "no /Measure here");
    assert!(
        matches!(r.appearance, AppearanceWrite::InPlace(_)),
        "pdfcer's own unshared /AP is rewritten in place: {:?}",
        r.appearance
    );

    let after = reload(&s);
    let d = dict_of(&after, id);
    let v = pairs(&after, &d, b"Vertices");
    assert_eq!(v.len(), 4);
    assert!(
        close(v[2].0, 230.0) && close(v[2].1, 190.0),
        "vertex 2 moved: {:?}",
        v[2]
    );
    for i in [0, 1, 3] {
        assert_eq!(v[i], QUAD[i], "vertex {i} untouched");
    }
    // /Rect follows: the hull grew to the right and shrank at the top.
    let rect = nums(&after, &d, b"Rect");
    assert!(
        rect[2] > 230.0,
        "urx encloses the moved vertex plus stroke: {rect:?}"
    );
    assert!(
        rect[3] < 201.0 + 1e-6 && rect[3] > 190.0,
        "ury follows the new hull: {rect:?}"
    );
    assert_eq!(r.rect_after.urx, rect[2]);
    // Keys the verb does not own survive verbatim.
    assert_eq!(
        d.get(b"AP"),
        ap_before.as_ref(),
        "/AP entry (same stream object) unchanged"
    );
    assert!(d.get(b"Contents").is_some() && d.get(b"T").is_some());
    assert_eq!(
        d.get(b"M"),
        Some(&Object::String(b"D:20260905120000Z".to_vec())),
        "/M left exactly as it was"
    );
}

#[test]
fn inserting_a_vertex_lands_after_the_named_index() {
    let (mut s, id) = with_markup(&polygon(TRI.to_vec()));
    let r = s
        .insert_annotation_vertex(id, 1, Point::new(220.0, 140.0))
        .expect("insert");
    assert_eq!((r.vertices_before, r.vertices_after), (3, 4));
    let after = reload(&s);
    let v = pairs(&after, &dict_of(&after, id), b"Vertices");
    assert_eq!(
        v,
        vec![TRI[0], TRI[1], (220.0, 140.0), TRI[2]],
        "new vertex sits at index after+1"
    );
}

#[test]
fn removing_a_vertex_and_the_polygon_floor_of_three() {
    let (mut s, id) = with_markup(&polygon(QUAD.to_vec()));
    let r = s.remove_annotation_vertex(id, 0).expect("4 -> 3 is fine");
    assert_eq!((r.vertices_before, r.vertices_after), (4, 3));
    let after = reload(&s);
    let v = pairs(&after, &dict_of(&after, id), b"Vertices");
    assert_eq!(v, vec![QUAD[1], QUAD[2], QUAD[3]]);

    // 3 -> 2 would leave a line pretending to be an area: refused by name.
    let err = s.remove_annotation_vertex(id, 0).expect_err("at the floor");
    match err {
        EditError::ReshapeWouldBreachVertexFloor {
            remaining,
            minimum,
            subtype,
            ..
        } => {
            assert_eq!((remaining, minimum), (2, 3));
            assert_eq!(subtype, "Polygon");
        }
        other => panic!("expected the floor refusal, got {other:?}"),
    }
    // And the refusal wrote nothing.
    let again = reload(&s);
    assert_eq!(pairs(&again, &dict_of(&again, id), b"Vertices").len(), 3);
}

#[test]
fn a_polyline_floor_is_two_not_three() {
    let (mut s, id) = with_markup(&polyline(TRI.to_vec()));
    s.remove_annotation_vertex(id, 1)
        .expect("3 -> 2 on an open path");
    let err = s
        .remove_annotation_vertex(id, 0)
        .expect_err("2 -> 1 refused");
    assert!(
        matches!(
            err,
            EditError::ReshapeWouldBreachVertexFloor {
                remaining: 1,
                minimum: 2,
                ..
            }
        ),
        "{err:?}"
    );
}

#[test]
fn an_index_that_names_nothing_is_refused_for_every_operation() {
    let (mut s, id) = with_markup(&polygon(TRI.to_vec()));
    for edit in [
        VertexEdit::Move {
            index: 3,
            dx: 1.0,
            dy: 1.0,
        },
        VertexEdit::Insert {
            after: 3,
            at: Point::new(0.0, 0.0),
        },
        VertexEdit::Remove { index: 7 },
    ] {
        let err = s
            .reshape_annotation(id, edit, None)
            .expect_err("out of range");
        assert!(
            matches!(
                err,
                EditError::AnnotationVertexIndexOutOfRange { count: 3, .. }
            ),
            "{edit:?} -> {err:?}"
        );
    }
}

#[test]
fn a_non_finite_result_is_refused_before_anything_is_staged() {
    let (mut s, id) = with_markup(&polygon(TRI.to_vec()));
    let depth = s.undo_depth();
    let err = s
        .move_annotation_vertex(id, 0, f64::NAN, 0.0)
        .expect_err("NaN");
    assert!(
        matches!(err, EditError::AnnotationVertexNotPlaceable { .. }),
        "{err:?}"
    );
    let err = s
        .insert_annotation_vertex(id, 0, Point::new(f64::INFINITY, 1.0))
        .expect_err("inf");
    assert!(
        matches!(err, EditError::AnnotationVertexNotPlaceable { .. }),
        "{err:?}"
    );
    assert_eq!(s.undo_depth(), depth, "nothing was committed");
}

// ---------------------------------------------------------------------------
// 3. Line — move only
// ---------------------------------------------------------------------------

#[test]
fn a_line_moves_either_endpoint_and_refuses_insert_and_remove_by_name() {
    let (mut s, id) = with_markup(&line());
    s.move_annotation_vertex(id, 1, 10.0, 5.0)
        .expect("end moves");
    s.move_annotation_vertex(id, 0, -5.0, 0.0)
        .expect("start moves");
    let after = reload(&s);
    let d = dict_of(&after, id);
    assert_eq!(nums(&after, &d, b"L"), vec![45.0, 50.0, 260.0, 125.0]);
    // The arrowhead survived the re-bake — /LE is re-asserted by the bake.
    assert!(d.get(b"LE").is_some(), "/LE kept");

    let err = s
        .move_annotation_vertex(id, 2, 1.0, 1.0)
        .expect_err("only 0/1");
    assert!(
        matches!(
            err,
            EditError::AnnotationVertexIndexOutOfRange { count: 2, .. }
        ),
        "{err:?}"
    );
    for (edit, want) in [
        (
            VertexEdit::Insert {
                after: 0,
                at: Point::new(100.0, 100.0),
            },
            VertexEditKind::Inserted,
        ),
        (VertexEdit::Remove { index: 0 }, VertexEditKind::Removed),
    ] {
        let err = s.reshape_annotation(id, edit, None).expect_err("refused");
        match err {
            EditError::GeometryNotReshapable {
                edit,
                reason,
                subtype,
                ..
            } => {
                assert_eq!(edit, want);
                assert_eq!(subtype, "Line");
                assert!(
                    reason.contains("PolyLine"),
                    "the reason names the remedy: {reason}"
                );
            }
            other => panic!("expected a named refusal, got {other:?}"),
        }
    }
}

// ---------------------------------------------------------------------------
// 4. Ink, Square, text markup — refused by name, never silently
// ---------------------------------------------------------------------------

#[test]
fn ink_square_and_text_markup_refuse_every_operation_with_a_stated_reason() {
    let specs: Vec<(MarkupSpec, &str)> = vec![
        (ink(), "Ink"),
        (
            MarkupSpec::Circle {
                rect: pdfcer_core::page_tree::Rect {
                    llx: 10.0,
                    lly: 10.0,
                    urx: 50.0,
                    ury: 50.0,
                },
                border: Some(Color::Gray(0.0)),
                interior: None,
                border_width: 1.0,
            },
            "Circle",
        ),
        (
            MarkupSpec::TextMarkup {
                kind: TextMarkupKind::Highlight,
                quads: vec![pdfcer_core::annot_author::Quad {
                    ul: (10.0, 30.0),
                    ur: (100.0, 30.0),
                    ll: (10.0, 10.0),
                    lr: (100.0, 10.0),
                }],
                color: Color::Rgb(1.0, 1.0, 0.0),
            },
            "Highlight",
        ),
    ];
    for (spec, subtype) in specs {
        let (mut s, id) = with_markup(&spec);
        let depth = s.undo_depth();
        for edit in [
            VertexEdit::Move {
                index: 0,
                dx: 1.0,
                dy: 1.0,
            },
            VertexEdit::Insert {
                after: 0,
                at: Point::new(1.0, 1.0),
            },
            VertexEdit::Remove { index: 0 },
        ] {
            // The preview and the verb give the SAME named answer.
            let preview = s
                .reshape_annotation_preview(id, edit)
                .expect_err("preview refuses");
            let err = s
                .reshape_annotation(id, edit, None)
                .expect_err("verb refuses");
            for e in [preview, err] {
                match e {
                    EditError::GeometryNotReshapable {
                        subtype: got,
                        reason,
                        ..
                    } => {
                        assert_eq!(got, subtype);
                        assert!(!reason.is_empty());
                    }
                    other => panic!("{subtype} {edit:?}: expected a named refusal, got {other:?}"),
                }
            }
        }
        assert_eq!(s.undo_depth(), depth, "{subtype}: a refusal writes nothing");
    }
}

// ---------------------------------------------------------------------------
// 5. Cloud — one bake, /BE survives, /Rect is the bulged outline
// ---------------------------------------------------------------------------

#[test]
fn a_reshaped_cloud_bakes_byte_identically_to_a_freshly_authored_one() {
    let (mut s, id) = with_markup(&cloud(QUAD.to_vec()));
    s.move_annotation_vertex(id, 2, 40.0, 25.0)
        .expect("move a cloud vertex");
    let edited = reload(&s);
    let d = dict_of(&edited, id);
    assert!(d.get(b"BE").is_some(), "/BE survives a reshape");
    let edited_ap = ap_bytes(&edited, &d);
    let edited_rect = nums(&edited, &d, b"Rect");
    let edited_v = pairs(&edited, &d, b"Vertices");

    // Delete-and-redraw with the same vertices, through `add_markup`.
    let mut moved = QUAD.to_vec();
    moved[2] = (240.0, 225.0);
    assert_eq!(edited_v, moved);
    let (fresh_s, fresh_id) = with_markup(&cloud(moved));
    let fresh = reload(&fresh_s);
    let fd = dict_of(&fresh, fresh_id);
    assert_eq!(
        edited_ap,
        ap_bytes(&fresh, &fd),
        "edit-in-place and delete-and-redraw must draw the same scallops"
    );
    assert_eq!(edited_rect, nums(&fresh, &fd, b"Rect"));

    // And the rectangle is the bulged outline, not the vertex hull: a
    // cloudy /Polygon has no /RD to say otherwise.
    assert!(
        edited_rect[0] < 100.0 && edited_rect[1] < 100.0,
        "bulges below/left: {edited_rect:?}"
    );
    assert!(
        edited_rect[2] > 240.0 && edited_rect[3] > 225.0,
        "bulges above/right: {edited_rect:?}"
    );
    assert!(fd.get(b"RD").is_none(), "no /RD is written on a Polygon");
}

// ---------------------------------------------------------------------------
// 6. The two lock gates
// ---------------------------------------------------------------------------

#[test]
fn locked_refuses_a_reshape_and_locked_contents_does_not() {
    const LOCKED: u32 = 128;
    const LOCKED_CONTENTS: u32 = 512;

    let annot = ObjId::new(4, 0);
    let mut s = EditSession::new(Document::from_bytes(pdf_with_flagged_polygon(LOCKED)).unwrap());
    let err = s
        .move_annotation_vertex(annot, 0, 1.0, 1.0)
        .expect_err("Locked");
    assert!(matches!(err, EditError::AnnotationLocked { .. }), "{err:?}");

    let mut s =
        EditSession::new(Document::from_bytes(pdf_with_flagged_polygon(LOCKED_CONTENTS)).unwrap());
    let r = s
        .move_annotation_vertex(annot, 0, 1.0, 1.0)
        .expect("LockedContents guards the comment text, not the geometry");
    assert!(
        matches!(r.appearance, AppearanceWrite::Created(_)),
        "a hand-written annotation with no /AP gets one: {:?}",
        r.appearance
    );
    let after = reload(&s);
    let d = dict_of(&after, annot);
    assert_eq!(pairs(&after, &d, b"Vertices")[0], (101.0, 101.0));
    assert_eq!(
        d.get(b"F"),
        Some(&Object::Integer(i64::from(LOCKED_CONTENTS))),
        "the flag itself is untouched"
    );
}

// ---------------------------------------------------------------------------
// 7. Undo, the command kind, and the ce-dimension refusal
// ---------------------------------------------------------------------------

#[test]
fn undo_restores_vertices_rect_and_appearance_on_the_same_object() {
    let (mut s, id) = with_markup(&polygon(QUAD.to_vec()));
    let before = reload(&s);
    let bd = dict_of(&before, id);
    let (v0, r0, ap0) = (
        pairs(&before, &bd, b"Vertices"),
        nums(&before, &bd, b"Rect"),
        ap_bytes(&before, &bd),
    );

    s.insert_annotation_vertex(id, 3, Point::new(50.0, 150.0))
        .unwrap();
    assert_eq!(
        s.undo_kind(),
        Some(CommandKind::ReshapeAnnotation {
            edit: VertexEditKind::Inserted
        })
    );
    s.undo().expect("undo");

    let after = reload(&s);
    let ad = dict_of(&after, id);
    assert_eq!(pairs(&after, &ad, b"Vertices"), v0);
    assert_eq!(nums(&after, &ad, b"Rect"), r0);
    assert_eq!(ap_bytes(&after, &ad), ap0);
}

#[test]
fn a_ce_dimension_is_sent_to_its_own_verbs() {
    let mut s = session("annot/demo-annotated.pdf");
    let (annot_id, _dim) = s
        .add_dimension(
            0,
            DEFAULT_GROUP_ID,
            DimensionKind::Linear {
                a: Point::new(100.0, 200.0),
                b: Point::new(300.0, 200.0),
                constraint: AxisConstraint::Horizontal,
                offset: 0.0,
                text_along: 0.0,
            },
        )
        .expect("author a ce dimension");
    let err = s
        .move_annotation_vertex(annot_id, 0, 5.0, 0.0)
        .expect_err("a ce dimension is a /Line, but not this verb's");
    assert!(
        matches!(err, EditError::AnnotationIsCeDimension { .. }),
        "{err:?}"
    );
}

#[test]
fn the_full_verb_stamps_m_when_given_a_date() {
    let (mut s, id) = with_markup(&polygon(TRI.to_vec()));
    let r = s
        .reshape_annotation(
            id,
            VertexEdit::Move {
                index: 0,
                dx: 1.0,
                dy: 0.0,
            },
            Some("D:20260905130000Z"),
        )
        .unwrap();
    assert!(r.mod_date_written);
    let after = reload(&s);
    assert_eq!(
        dict_of(&after, id).get(b"M"),
        Some(&Object::String(b"D:20260905130000Z".to_vec()))
    );
}

/// The preview forecasts exactly what the verb then does.
#[test]
fn the_preview_matches_the_verb() {
    let (mut s, id) = with_markup(&polygon(QUAD.to_vec()));
    let edit = VertexEdit::Remove { index: 1 };
    let depth = s.undo_depth();
    let f = s.reshape_annotation_preview(id, edit).expect("preview");
    assert_eq!(s.undo_depth(), depth, "a preview writes nothing");
    let r = s.reshape_annotation(id, edit, None).expect("verb");
    assert_eq!(f.vertices_after, r.vertices_after);
    assert_eq!(f.rect_after, r.rect_after);
    assert_eq!(f.rect_before, r.rect_before);
    assert_eq!(f.edit, r.edit);
    assert_eq!(f.subtype, r.subtype);
}
