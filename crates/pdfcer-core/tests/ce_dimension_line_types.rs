//! Named drafting line types and the line-type scale (pdfcer-gui request
//! G331): a name is stored, not its runs, and re-derived per standard,
//! width and scale; the factory centre line is unchanged byte for byte.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::dimension::{
    DEFAULT_GROUP_ID, DimDash, DimStandard, DimensionId, DimensionKind, DimensionStyle, GroupStyle,
    LineType, LineTypeRef, PartStroke, ResolvedStroke, StyleOverrides, StyleSource, resolve_style,
    serialize_model, sidecar_version, style_provenance,
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

fn one() -> (EditSession, ObjId, DimensionId) {
    let mut s = hello();
    let kind = DimensionKind::Linear {
        a: Point::new(100.0, 200.0),
        b: Point::new(300.0, 200.0),
        constraint: AxisConstraint::Horizontal,
        offset: 0.0,
        text_along: 0.0,
        extension_gap: [None; 2],
    };
    let (annot, id) = s.add_dimension(0, DEFAULT_GROUP_ID, kind).unwrap();
    (s, annot, id)
}

fn save(s: &EditSession) -> Vec<u8> {
    s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0
}

fn ap(s: &EditSession, id: ObjId) -> String {
    let doc = Document::from_bytes(save(s)).unwrap();
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

fn style_of(s: &EditSession, id: DimensionId) -> DimensionStyle {
    let model = s.dimension_model();
    let rec = model.dimension(id).unwrap();
    resolve_style(model.group(rec.group).unwrap(), &rec.style)
}

fn named(t: LineType) -> Option<LineTypeRef> {
    Some(LineTypeRef::Named(t))
}

/// The driven check: a named type reaches the `/AP` as its table's runs,
/// and a scale or a change of standard re-derives them.
#[test]
fn a_named_type_draws_its_runs_and_rederives_on_scale_and_standard() {
    let (mut s, annot, id) = one();
    let over = StyleOverrides {
        dash: named(LineType::Hidden),
        ..StyleOverrides::default()
    };
    s.set_dimension_style(id, over).unwrap();
    assert!(ap(&s, annot).contains("[9 4.5] 0 d"), "{}", ap(&s, annot));
    let scaled = StyleOverrides {
        line_type_scale: Some(2.0),
        ..over
    };
    s.set_dimension_style(id, scaled).unwrap();
    assert!(ap(&s, annot).contains("[18 9] 0 d"), "{}", ap(&s, annot));
    let iso = StyleOverrides {
        standard: Some(DimStandard::Iso),
        line_width: Some(0.5),
        line_type_scale: None,
        ..over
    };
    s.set_dimension_style(id, iso).unwrap();
    assert_eq!(style_of(&s, id).dash.pattern(), &[6.0, 1.5]);
    assert!(ap(&s, annot).contains("[6 1.5] 0 d"), "{}", ap(&s, annot));
}

/// Would catch: the factory centre pattern changing when it moved onto the
/// line-type table, which would rewrite every centre line on regeneration.
#[test]
fn the_factory_centre_line_is_the_center_type_unchanged() {
    let (s, _, id) = one();
    let ansi = style_of(&s, id);
    assert_eq!(ansi.centre_line_dash().pattern(), &[54.0, 9.0, 9.0, 9.0]);
    let iso = DimensionStyle {
        standard: DimStandard::Iso,
        line_width: 0.5,
        ..ansi
    };
    assert_eq!(iso.centre_line_dash().pattern(), &[12.0, 1.5, 0.5, 1.5]);
    assert_eq!(iso.centre_line_type(), LineTypeRef::Named(LineType::Center));
}

/// Would catch: a followed named type drawn at the lines' width on a circle
/// that has its own, so ISO proportions would be wrong on the circle.
#[test]
fn a_followed_named_type_is_rederived_at_the_circle_width() {
    let (s, _, id) = one();
    let model = s.dimension_model();
    let group = model.group(model.dimension(id).unwrap().group).unwrap();
    let over = StyleOverrides {
        dash: named(LineType::Hidden),
        standard: Some(DimStandard::Iso),
        line_width: Some(1.0),
        circle: PartStroke::FOLLOW.with_width(Some(2.0)),
        ..StyleOverrides::default()
    };
    let style = resolve_style(group, &over);
    assert_eq!(style.dash.pattern(), &[12.0, 3.0]);
    assert_eq!(style.circle_stroke().dash.pattern(), &[24.0, 6.0]);
    assert_eq!(
        style.circle_line_type(),
        LineTypeRef::Named(LineType::Hidden)
    );
}

#[test]
fn names_survive_save_with_provenance_and_version_10() {
    let (mut s, _, id) = one();
    let g = GroupStyle {
        dash: named(LineType::Phantom),
        ..GroupStyle::default()
    };
    s.set_group_style(DEFAULT_GROUP_ID, g).unwrap();
    let over = StyleOverrides {
        line_type_scale: Some(1.5),
        circle: PartStroke::FOLLOW.with_line_type(named(LineType::Stitch)),
        ..StyleOverrides::default()
    };
    s.set_dimension_style(id, over).unwrap();
    let back = EditSession::new(Document::from_bytes(save(&s)).unwrap()).dimension_model();
    let group = back.group(DEFAULT_GROUP_ID).unwrap();
    assert_eq!(group.style.dash, named(LineType::Phantom));
    let rec = back.dimension(id).unwrap();
    assert_eq!(rec.style.line_type_scale, Some(1.5));
    assert_eq!(rec.style.circle.dash, named(LineType::Stitch));
    let p = style_provenance(group, &rec.style);
    assert_eq!(p.dash, StyleSource::Group);
    assert_eq!(p.line_type_scale, StyleSource::Dimension);
    assert_eq!(p.each().len(), 27);
    assert_eq!(sidecar_version(&serialize_model(&back)), Some(10));
}

/// Would catch: a document with only custom runs bumped to version 10,
/// locking an older build out of a file it reads perfectly.
#[test]
fn custom_runs_alone_stay_below_version_10() {
    let (mut s, _, id) = one();
    let over = StyleOverrides {
        dash: DimDash::new(&[3.0, 1.5]).map(LineTypeRef::from),
        ..StyleOverrides::default()
    };
    s.set_dimension_style(id, over).unwrap();
    let back = EditSession::new(Document::from_bytes(save(&s)).unwrap()).dimension_model();
    assert!(sidecar_version(&serialize_model(&back)) < Some(10));
}

#[test]
fn a_bad_scale_is_refused_before_any_change() {
    let (mut s, _, id) = one();
    for bad in [0.0, -1.0, f64::INFINITY] {
        let over = StyleOverrides {
            line_type_scale: Some(bad),
            ..StyleOverrides::default()
        };
        let err = s.set_dimension_style(id, over).unwrap_err();
        assert!(
            matches!(err, EditError::LineTypeScaleOutOfRange { .. }),
            "{err}"
        );
        let g = GroupStyle {
            line_type_scale: Some(bad),
            ..GroupStyle::default()
        };
        assert!(s.set_group_style(DEFAULT_GROUP_ID, g).is_err());
    }
    let rec = s.dimension_model();
    assert_eq!(rec.dimension(id).unwrap().style.line_type_scale, None);
}

/// `LineType` and `PartStroke` are `#[non_exhaustive]`: built, matched and
/// resolved from outside the crate. Chain Thick is absent from `ALL`.
#[test]
fn line_types_are_used_outside_the_crate() {
    assert_eq!(LineType::ALL.len(), 6);
    assert!(LineType::ALL.iter().all(|t| !t.token().contains("thick")));
    let described = match LineType::ALL[2] {
        LineType::Phantom => "phantom",
        _ => "other",
    };
    assert_eq!(described, "phantom");
    let part = PartStroke::FOLLOW.with_line_type(named(LineType::Stitch));
    let main = ResolvedStroke::new(
        DimDash::SOLID,
        2.0,
        pdfcer_core::annot_author::Color::Gray(0.0),
    );
    assert_eq!(
        part.resolve(main, DimStandard::Iso, 1.0).dash.pattern(),
        &[6.0, 6.0]
    );
    assert_eq!(
        LineTypeRef::from(DimDash::SOLID).line_type(),
        Some(LineType::Continuous)
    );
    assert_eq!(LineType::parse("CENTRE"), Some(LineType::Center));
    assert_eq!(LineType::parse("chain-thick"), None);
}
