//! Centre lines (`Pass 739.0`) and centre marks (`Pass 740.0`) as ce
//! dimensions: authored, saved, reopened, moved, deleted and undone through
//! the public `EditSession` surface.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::dimension::{
    CentreLineRefusal, CentreMarkSize, CentreMarkSpec, CentreSnapRole, DEFAULT_GROUP_ID,
    DimensionId, DimensionKind, FitCircle, GroupStyle, author_centre_line,
};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::object::{Dict, ObjId, Object};
use pdfcer_core::vector::linepick::{ParallelPolicy, PickedLine, TwoLineRelation};
use pdfcer_core::vector::{HitTarget, Point};
use pdfcer_core::writer::SaveOptions;

/// One page plus an untouched content stream (object 4), so the byte check
/// has something to compare.
fn pdf() -> Vec<u8> {
    let content = "0 0 m 10 10 l S";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Contents 4 0 R /Resources << >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
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

fn line(sx: f64, sy: f64, ex: f64, ey: f64) -> PickedLine {
    PickedLine {
        target: HitTarget::Object(0),
        subpath: 0,
        segment: 0,
        start: Point::new(sx, sy),
        end: Point::new(ex, ey),
        pick: Point::new(f64::midpoint(sx, ex), f64::midpoint(sy, ey)),
    }
}

fn policy() -> ParallelPolicy {
    ParallelPolicy::from_setting(0.5)
}

fn parallel_centre_line() -> DimensionKind {
    author_centre_line(
        &line(100.0, 100.0, 300.0, 100.0),
        &line(150.0, 140.0, 350.0, 140.0),
        policy(),
        9.0,
    )
    .unwrap()
    .kind
}

fn circle() -> FitCircle {
    FitCircle {
        center: Point::new(200.0, 200.0),
        radius: 50.0,
        residual: 0.0,
    }
}

fn session() -> EditSession {
    EditSession::new(Document::from_bytes(pdf()).unwrap())
}

fn kind(s: &EditSession, id: DimensionId) -> Option<DimensionKind> {
    s.dimension_model().dimension(id).map(|d| d.kind.clone())
}

fn save(s: &EditSession) -> Vec<u8> {
    s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0
}

/// The annotation dictionary and its decoded `/AP /N` content, as saved.
fn saved(bytes: Vec<u8>, id: ObjId) -> (Dict, String) {
    let doc = Document::from_bytes(bytes).unwrap();
    let Object::Dict(annot) = doc.get(id).unwrap().value.clone() else {
        panic!("not a dict");
    };
    let Object::Dict(ap) = doc.resolve(annot.get(b"AP").unwrap()) else {
        panic!("no /AP");
    };
    let Object::Stream(st) = doc.resolve(ap.get(b"N").unwrap()) else {
        panic!("no /N");
    };
    let raw = st.data_span.slice(doc.bytes()).unwrap();
    let text =
        String::from_utf8_lossy(&pdfcer_core::filters::decode_stream(&st.dict, raw).unwrap())
            .into_owned();
    (annot, text)
}

fn name(d: &Dict, key: &[u8]) -> Vec<u8> {
    d.get(key)
        .and_then(Object::as_name)
        .map(|n| n.as_bytes().to_vec())
        .unwrap_or_default()
}

fn ink_strokes(d: &Dict) -> Vec<Vec<f64>> {
    d.get(b"InkList")
        .and_then(Object::as_array)
        .unwrap()
        .iter()
        .map(|s| {
            s.as_array()
                .unwrap()
                .iter()
                .filter_map(Object::as_number)
                .collect()
        })
        .collect()
}

#[test]
fn parallel_lines_get_a_midline_over_the_overlap_plus_overshoot() {
    let authored = author_centre_line(
        &line(100.0, 100.0, 300.0, 100.0),
        &line(150.0, 140.0, 350.0, 140.0),
        policy(),
        9.0,
    )
    .unwrap();
    let DimensionKind::CentreLine { a, b } = authored.kind else {
        panic!("not a centre line")
    };
    assert_eq!((a.x, a.y, b.x, b.y), (141.0, 120.0, 309.0, 120.0));
    assert!((authored.overlap - 150.0).abs() < 1e-9);
    assert!(matches!(
        authored.relation,
        TwoLineRelation::Parallel { .. }
    ));
    assert!(!authored.forced_parallel);
}

#[test]
fn angled_lines_get_the_bisector() {
    let authored = author_centre_line(
        &line(0.0, 0.0, 100.0, 0.0),
        &line(0.0, 0.0, 0.0, 100.0),
        policy(),
        9.0,
    )
    .unwrap();
    let DimensionKind::CentreLine { a, b } = authored.kind else {
        panic!("not a centre line")
    };
    let angle = (b.y - a.y).atan2(b.x - a.x).to_degrees();
    assert!((angle - 45.0).abs() < 1e-9, "{angle}");
    // Starts at the apex, not 9 pt past it in the opposite angle; the far
    // end overshoots as usual.
    assert!(a.x.abs() < 1e-9 && a.y.abs() < 1e-9, "{a:?}");
    let reach = 100.0 * std::f64::consts::FRAC_1_SQRT_2 + 9.0;
    assert!((b.x.hypot(b.y) - reach).abs() < 1e-9, "{b:?}");
    assert!(matches!(authored.relation, TwoLineRelation::Angled { .. }));
}

#[test]
fn forcing_parallel_still_reports_the_measured_angle() {
    let authored = author_centre_line(
        &line(0.0, 0.0, 200.0, 0.0),
        &line(0.0, 40.0, 200.0, 44.0),
        policy().forcing_parallel(),
        9.0,
    )
    .unwrap();
    assert!(authored.forced_parallel);
    assert!(authored.measured_angle_degrees.unwrap() > 1.0);
    assert!(matches!(
        authored.relation,
        TwoLineRelation::Parallel { .. }
    ));
}

#[test]
fn the_refusals_are_named() {
    let r = |a, b, o| author_centre_line(&a, &b, policy(), o).unwrap_err();
    assert_eq!(
        r(
            line(0.0, 0.0, 100.0, 0.0),
            line(150.0, 0.0, 250.0, 0.0),
            9.0
        ),
        CentreLineRefusal::Collinear
    );
    assert_eq!(
        r(
            line(0.0, 0.0, 100.0, 0.0),
            line(200.0, 40.0, 300.0, 40.0),
            9.0
        ),
        CentreLineRefusal::NoOverlap
    );
    assert_eq!(
        r(line(5.0, 5.0, 5.0, 5.0), line(0.0, 40.0, 100.0, 40.0), 9.0),
        CentreLineRefusal::Degenerate
    );
    assert_eq!(
        r(
            line(0.0, 0.0, 100.0, 0.0),
            line(0.0, 40.0, 100.0, 40.0),
            -1.0
        ),
        CentreLineRefusal::BadOvershoot
    );
    // Every refusal has a sentence an operator can read.
    assert!(!CentreLineRefusal::NoOverlap.to_string().contains("  "));
}

#[test]
fn a_degenerate_centre_line_or_mark_is_refused_before_anything_is_written() {
    let mut s = session();
    let p = Point::new(10.0, 10.0);
    let err = s
        .add_dimension(
            0,
            DEFAULT_GROUP_ID,
            DimensionKind::CentreLine { a: p, b: p },
        )
        .unwrap_err();
    assert!(matches!(err, EditError::CentreGeometryDegenerate { .. }));
    let mut flat = circle();
    flat.radius = 0.0;
    let err = s
        .add_dimension(
            0,
            DEFAULT_GROUP_ID,
            DimensionKind::CentreMark {
                fit: flat,
                mark: CentreMarkSpec::default(),
            },
        )
        .unwrap_err();
    assert!(matches!(err, EditError::CentreGeometryDegenerate { .. }));
    assert_eq!(s.undo_depth(), 0);
}

#[test]
fn a_centre_line_saves_reopens_and_reads_back_as_a_centre_line() {
    let mut s = session();
    let (annot, id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, parallel_centre_line())
        .unwrap();
    let bytes = save(&s);
    let reopened = EditSession::new(Document::from_bytes(bytes.clone()).unwrap());
    assert_eq!(kind(&reopened, id), Some(parallel_centre_line()));

    let (dict, ap) = saved(bytes, annot);
    assert_eq!(name(&dict, b"Subtype"), b"Line");
    assert!(dict.get(b"IT").is_none(), "a centre line measures nothing");
    assert!(dict.get(b"Measure").is_none());
    // Long-short-long: four runs, the first the longest.
    let dash_line = ap.lines().find(|l| l.ends_with(" d")).expect("a dash");
    let runs: Vec<f64> = dash_line
        .trim_start_matches('[')
        .split(']')
        .next()
        .unwrap()
        .split_whitespace()
        .map(|t| t.parse().unwrap())
        .collect();
    assert_eq!(runs.len(), 4, "{dash_line}");
    assert!(runs[0] > runs[1] && runs[0] > runs[2], "{runs:?}");
}

#[test]
fn a_centre_line_is_a_snap_target_at_its_ends_and_midpoint() {
    let mut s = session();
    let (_, id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, parallel_centre_line())
        .unwrap();
    let model = s.dimension_model();
    let record = model.dimension(id).unwrap();
    let snaps = record.centre_snap_points(model.group(record.group).unwrap());
    let at = |role| {
        snaps
            .iter()
            .filter(|p| p.role == role)
            .map(|p| (p.point.x, p.point.y))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        at(CentreSnapRole::LineEnd),
        vec![(141.0, 120.0), (309.0, 120.0)]
    );
    assert_eq!(at(CentreSnapRole::LineMidpoint), vec![(225.0, 120.0)]);
}

#[test]
fn a_centre_line_moves_deletes_and_undoes_as_one_step_each() {
    let mut s = session();
    let (_, id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, parallel_centre_line())
        .unwrap();
    assert_eq!(s.undo_depth(), 1, "authoring is one undo step");

    s.move_dimension(id, 10.0, -5.0).unwrap();
    let Some(DimensionKind::CentreLine { a, b }) = kind(&s, id) else {
        panic!("moved into something else")
    };
    assert_eq!((a.x, a.y, b.x, b.y), (151.0, 115.0, 319.0, 115.0));
    s.undo();
    assert_eq!(kind(&s, id), Some(parallel_centre_line()));

    s.delete_dimension(id).unwrap();
    assert_eq!(kind(&s, id), None);
    s.undo();
    assert_eq!(kind(&s, id), Some(parallel_centre_line()));
}

#[test]
fn a_centre_line_has_no_label_to_edit_or_place() {
    let mut s = session();
    let (_, id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, parallel_centre_line())
        .unwrap();
    assert!(matches!(
        s.set_dimension_label(id, Some("CL")),
        Err(EditError::CentreGeometryHasNoLabel { .. })
    ));
    assert!(matches!(
        s.place_dimension(id, 5.0, 0.0),
        Err(EditError::CentreGeometryHasNoLabel { .. })
    ));
}

#[test]
fn a_standalone_centre_mark_is_an_ink_cross_and_reads_back() {
    let mut s = session();
    let mark = DimensionKind::CentreMark {
        fit: circle(),
        mark: CentreMarkSpec::default(),
    };
    let (annot, id) = s.add_dimension(0, DEFAULT_GROUP_ID, mark.clone()).unwrap();
    let bytes = save(&s);
    let reopened = EditSession::new(Document::from_bytes(bytes.clone()).unwrap());
    assert_eq!(kind(&reopened, id), Some(mark));

    let (dict, _) = saved(bytes, annot);
    assert_eq!(name(&dict, b"Subtype"), b"Ink");
    // Factory size is half the 10 pt text height: arms of 5 either side.
    assert_eq!(
        ink_strokes(&dict),
        vec![
            vec![205.0, 200.0, 195.0, 200.0],
            vec![200.0, 205.0, 200.0, 195.0]
        ]
    );
}

#[test]
fn extension_lines_are_off_by_default_and_run_past_the_rim_when_on() {
    let mut s = session();
    let mark = DimensionKind::CentreMark {
        fit: circle(),
        mark: CentreMarkSpec {
            extension_lines: true,
            angle: 0.0,
        },
    };
    let (annot, _) = s.add_dimension(0, DEFAULT_GROUP_ID, mark).unwrap();
    let (dict, _) = saved(save(&s), annot);
    let strokes = ink_strokes(&dict);
    assert_eq!(strokes.len(), 6);
    // Each extension line starts 2x the arm from the centre and ends one arm
    // past the rim.
    assert!(
        strokes.contains(&vec![210.0, 200.0, 255.0, 200.0]),
        "{strokes:?}"
    );
    assert!(!CentreMarkSpec::default().extension_lines);
}

#[test]
fn the_group_centre_mark_size_sets_the_arm() {
    let mut s = session();
    let mark = DimensionKind::CentreMark {
        fit: circle(),
        mark: CentreMarkSpec::default(),
    };
    let (annot, _) = s.add_dimension(0, DEFAULT_GROUP_ID, mark).unwrap();
    s.set_group_style(
        DEFAULT_GROUP_ID,
        GroupStyle {
            centre_mark_size: Some(CentreMarkSize::Fixed(12.0)),
            ..GroupStyle::default()
        },
    )
    .unwrap();
    let (dict, _) = saved(save(&s), annot);
    assert_eq!(ink_strokes(&dict)[0], vec![212.0, 200.0, 188.0, 200.0]);
    assert_eq!(
        CentreMarkSize::parse("text:0.5"),
        Some(CentreMarkSize::default())
    );
}

#[test]
fn a_diameter_with_its_centre_mark_is_one_object() {
    let mut s = session();
    let dia = DimensionKind::Circular {
        fit: circle(),
        show_diameter: true,
        leader_angle: 0.0,
        text_distance: None,
        centre_mark: Some(CentreMarkSpec::default()),
    };
    let (annot, id) = s.add_dimension(0, DEFAULT_GROUP_ID, dia.clone()).unwrap();
    assert_eq!(s.dimension_model().dimensions().len(), 1);
    let bytes = save(&s);
    let reopened = EditSession::new(Document::from_bytes(bytes.clone()).unwrap());
    assert_eq!(kind(&reopened, id), Some(dia.clone()));
    let (dict, ap) = saved(bytes, annot);
    assert_eq!(name(&dict, b"IT"), b"LineDimension", "it still measures");
    assert!(
        ap.contains("205 200 m\n195 200 l"),
        "the cross is in the same /AP:\n{ap}"
    );

    // Rotating turns the mark with the leader; undo restores both.
    s.rotate_dimension(id, (200.0, 200.0), 30.0).unwrap();
    let Some(DimensionKind::Circular {
        centre_mark: Some(m),
        ..
    }) = kind(&s, id)
    else {
        panic!("lost its mark")
    };
    assert!((m.angle - 30.0).abs() < 1e-9, "{}", m.angle);
    s.undo();
    assert_eq!(kind(&s, id), Some(dia));
    s.delete_dimension(id).unwrap();
    assert!(s.dimension_model().dimensions().is_empty());
}

#[test]
fn an_incremental_save_leaves_untouched_objects_byte_identical() {
    let original = pdf();
    let mut s = session();
    s.add_dimension(0, DEFAULT_GROUP_ID, parallel_centre_line())
        .unwrap();
    s.add_dimension(
        0,
        DEFAULT_GROUP_ID,
        DimensionKind::CentreMark {
            fit: circle(),
            mark: CentreMarkSpec::default(),
        },
    )
    .unwrap();
    let bytes = save(&s);
    assert_eq!(&bytes[..original.len()], &original[..]);
    let appended = String::from_utf8_lossy(&bytes[original.len()..]).into_owned();
    for untouched in ["\n2 0 obj", "\n4 0 obj"] {
        assert!(!appended.contains(untouched), "{untouched} was rewritten");
    }
}
