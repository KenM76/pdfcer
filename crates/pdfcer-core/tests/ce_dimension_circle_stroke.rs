//! A radius/diameter ce dimension's circle: shown or hidden, and stroked
//! apart from its leader (pdfcer-gui request G329).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::annot_author::Color;
use pdfcer_core::dimension::{
    DEFAULT_GROUP_ID, DimDash, DimensionId, DimensionKind, FitCircle, GroupStyle, PartStroke,
    ResolvedStroke, StyleOverrides, StyleSource,
};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::vector::{AxisConstraint, Point};
use pdfcer_core::writer::SaveOptions;
use std::path::Path;

fn hello() -> EditSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/hello.pdf");
    EditSession::new(Document::load(&path).expect("load fixture"))
}

fn circular() -> DimensionKind {
    DimensionKind::Circular {
        fit: FitCircle {
            center: Point::new(300.0, 300.0),
            radius: 50.0,
            residual: 0.0,
        },
        show_diameter: true,
        show_circle: true,
        leader_angle: 30.0,
        text_distance: None,
        centre_mark: None,
    }
}

fn with_one() -> (EditSession, ObjId, DimensionId) {
    let mut s = hello();
    let (annot, id) = s.add_dimension(0, DEFAULT_GROUP_ID, circular()).unwrap();
    (s, annot, id)
}

fn save(s: &EditSession) -> Vec<u8> {
    s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0
}

/// The decoded `/AP /N` content of annotation `id` in `bytes`.
fn ap_of(bytes: Vec<u8>, id: ObjId) -> String {
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
    String::from_utf8_lossy(&pdfcer_core::filters::decode_stream(&st.dict, raw).unwrap())
        .into_owned()
}

fn ap(s: &EditSession, id: ObjId) -> String {
    ap_of(save(s), id)
}

fn curves(ap: &str) -> usize {
    ap.lines().filter(|l| l.ends_with(" c")).count()
}

fn shown(s: &EditSession, id: DimensionId) -> bool {
    match s.dimension_model().dimension(id).unwrap().kind {
        DimensionKind::Circular { show_circle, .. } => show_circle,
        _ => panic!("not circular"),
    }
}

fn restyle(s: &mut EditSession, id: DimensionId, circle: PartStroke) {
    let style = StyleOverrides {
        circle,
        ..StyleOverrides::default()
    };
    s.set_dimension_style(id, style).unwrap();
}

/// Would catch: an unrelated regeneration rewriting every circular `/AP`
/// because the circle now takes its own `q … Q` even when it matches.
#[test]
fn a_circle_stroked_like_its_leader_bakes_byte_identical() {
    let (mut s, annot, id) = with_one();
    let before = ap(&s, annot);
    let same = PartStroke::FOLLOW
        .with_width(Some(0.75))
        .with_color(Some(Color::Rgb(0.0, 0.0, 0.0)))
        .with_dash(Some(DimDash::SOLID));
    restyle(&mut s, id, same);
    assert_eq!(ap(&s, annot), before);
    assert_eq!(curves(&before), 4, "{before}");
}

#[test]
fn a_restyled_circle_strokes_in_its_own_q_before_the_leader() {
    let (mut s, annot, id) = with_one();
    let phantom = PartStroke::FOLLOW
        .with_dash(DimDash::new(&[6.0, 2.0]))
        .with_width(Some(0.35))
        .with_color(Some(Color::Rgb(0.0, 0.0, 1.0)));
    restyle(&mut s, id, phantom);
    let ap = ap(&s, annot);
    let lines: Vec<&str> = ap.lines().collect();
    let q = lines.iter().position(|l| *l == "q").expect("q");
    let first_curve = lines.iter().position(|l| l.ends_with(" c")).unwrap();
    let big_q = lines.iter().position(|l| *l == "Q").expect("Q");
    assert!(q < first_curve && first_curve < big_q, "{ap}");
    let state = lines[q..first_curve].join("\n");
    assert!(state.contains("0 0 1 RG"), "{state}");
    assert!(state.contains("0.35 w"), "{state}");
    assert!(state.contains("[6 2] 0 d"), "{state}");
    // The leader after `Q` keeps the ce dimension's solid black.
    assert!(!lines[big_q..].iter().any(|l| l.ends_with(" d")), "{ap}");
}

#[test]
fn a_hidden_circle_bakes_no_circle_and_survives_save_undo_and_redo() {
    let (mut s, annot, id) = with_one();
    let depth = s.undo_depth();
    s.set_circular_circle_shown(id, false).unwrap();
    assert_eq!(s.undo_depth(), depth + 1);
    assert_eq!(curves(&ap(&s, annot)), 0);

    let reopened = EditSession::new(Document::from_bytes(save(&s)).unwrap());
    assert!(!shown(&reopened, id));

    s.undo().unwrap();
    assert!(shown(&s, id));
    assert_eq!(curves(&ap(&s, annot)), 4);
    s.redo().unwrap();
    assert!(!shown(&s, id));
}

#[test]
fn setting_the_current_state_still_commits() {
    let (mut s, _, id) = with_one();
    let depth = s.undo_depth();
    s.set_circular_circle_shown(id, true).unwrap();
    assert_eq!(s.undo_depth(), depth + 1);
}

#[test]
fn a_non_circular_target_is_refused_before_any_change() {
    let mut s = hello();
    let linear = DimensionKind::Linear {
        a: Point::new(100.0, 200.0),
        b: Point::new(300.0, 200.0),
        constraint: AxisConstraint::Horizontal,
        offset: 0.0,
        text_along: 0.0,
        extension_gap: [None; 2],
    };
    let (_, id) = s.add_dimension(0, DEFAULT_GROUP_ID, linear).unwrap();
    let depth = s.undo_depth();
    let err = s.set_circular_circle_shown(id, false).unwrap_err();
    assert!(
        matches!(err, EditError::NotACircularDimension { .. }),
        "{err}"
    );
    assert_eq!(s.undo_depth(), depth);
    let err = s.set_circular_circle_shown(DimensionId(999), false);
    assert!(matches!(err, Err(EditError::DimensionNotFound { .. })));
}

#[test]
fn the_circle_stroke_survives_save_and_reopen_at_both_tiers() {
    let (mut s, _, id) = with_one();
    let group = GroupStyle {
        circle: PartStroke::FOLLOW.with_width(Some(0.25)),
        ..GroupStyle::default()
    };
    s.set_group_style(DEFAULT_GROUP_ID, group).unwrap();
    restyle(
        &mut s,
        id,
        PartStroke::FOLLOW.with_dash(DimDash::new(&[2.0, 2.0])),
    );
    let back = EditSession::new(Document::from_bytes(save(&s)).unwrap()).dimension_model();
    let g = back.group(DEFAULT_GROUP_ID).unwrap();
    assert_eq!(g.style.circle.width, Some(0.25));
    let rec = back.dimension(id).unwrap();
    assert_eq!(
        rec.style.circle.dash,
        DimDash::new(&[2.0, 2.0]).map(Into::into)
    );
    let p = pdfcer_core::dimension::style_provenance(g, &rec.style);
    assert_eq!(p.circle_width, StyleSource::Group);
    assert_eq!(p.circle_dash, StyleSource::Dimension);
    assert_eq!(p.circle_color, StyleSource::Factory);
}

#[test]
fn move_and_rotate_carry_the_hidden_circle_and_its_stroke() {
    let (mut s, _, id) = with_one();
    let stroke = PartStroke::FOLLOW.with_width(Some(2.0));
    restyle(&mut s, id, stroke);
    s.set_circular_circle_shown(id, false).unwrap();
    s.move_dimension(id, 10.0, -5.0).unwrap();
    assert!(!shown(&s, id));
    s.rotate_dimension(id, (300.0, 300.0), 90.0).unwrap();
    assert!(!shown(&s, id));
    assert_eq!(
        s.dimension_model().dimension(id).unwrap().style.circle,
        stroke
    );
}

/// `PartStroke` and `ResolvedStroke` are `#[non_exhaustive]`: a consumer
/// builds them through `FOLLOW`, the builders and `new`.
#[test]
fn part_strokes_are_built_outside_the_crate() {
    let p = PartStroke::FOLLOW.with_color(Some(Color::Gray(0.5)));
    assert_eq!(p.count(), 1);
    assert!(PartStroke::default().is_follow());
    let main = ResolvedStroke::new(DimDash::SOLID, 0.75, Color::Gray(0.0));
    let r = p.resolve(main, pdfcer_core::dimension::DimStandard::Ansi, 1.0);
    assert_eq!((r.width, r.color), (0.75, Color::Gray(0.5)));
    let lower = PartStroke::FOLLOW.with_width(Some(1.0)).with_color(None);
    assert_eq!(p.or(lower).width, Some(1.0));
}
