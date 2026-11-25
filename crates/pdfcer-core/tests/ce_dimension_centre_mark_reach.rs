//! A centre mark's extension-line gap, extension past the rim and line font
//! (pdfcer-gui request G333): the factory reproduces the arm-length
//! geometry byte for byte, the snaps move with the new geometry, and a gap
//! that leaves no room is disclosed as `extension_lines_drawn = false`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::dimension::{
    CentreMarkSize, CentreMarkSpec, CentreSnapRole, CentreStyle, DEFAULT_GROUP_ID, DimDash,
    DimensionId, DimensionKind, FitCircle, GroupStyle, PartStroke, StyleOverrides, StyleSource,
    resolve_style, sidecar_version, style_provenance,
};
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::vector::Point;
use pdfcer_core::writer::SaveOptions;
use std::path::Path;

const CENTRE: Point = Point::new(300.0, 300.0);
const RADIUS: f64 = 50.0;

fn hello() -> EditSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/hello.pdf");
    EditSession::new(Document::load(&path).expect("load fixture"))
}

fn mark_with_extensions() -> DimensionKind {
    DimensionKind::CentreMark {
        fit: FitCircle {
            center: CENTRE,
            radius: RADIUS,
            residual: 0.0,
        },
        mark: CentreMarkSpec {
            extension_lines: true,
            angle: 0.0,
        },
    }
}

fn one() -> (EditSession, ObjId, DimensionId) {
    let mut s = hello();
    let (annot, id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, mark_with_extensions())
        .unwrap();
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

fn restyle(s: &mut EditSession, id: DimensionId, centre: CentreStyle) {
    let style = StyleOverrides {
        centre,
        ..StyleOverrides::default()
    };
    s.set_dimension_style(id, style).unwrap();
}

fn style_of(s: &EditSession, id: DimensionId) -> pdfcer_core::dimension::DimensionStyle {
    let model = s.dimension_model();
    let rec = model.dimension(id).unwrap();
    resolve_style(model.group(rec.group).unwrap(), &rec.style)
}

/// Distances from the centre of every extension-line end snap.
fn extension_reach(s: &EditSession, id: DimensionId) -> Vec<f64> {
    let model = s.dimension_model();
    let rec = model.dimension(id).unwrap();
    rec.kind
        .centre_snap_points(style_of(s, id))
        .into_iter()
        .filter(|p| p.role == CentreSnapRole::ExtensionEnd)
        .map(|p| ((p.point.x - CENTRE.x).hypot(p.point.y - CENTRE.y) * 1e6).round() / 1e6)
        .collect()
}

/// Would catch: a factory gap or extension that is not the arm, which would
/// rewrite every existing mark's `/AP` on its next regeneration.
#[test]
fn factory_gap_and_extension_are_the_arm() {
    let (mut s, annot, id) = one();
    let before = ap(&s, annot);
    let arm = style_of(&s, id).centre_mark_arm();
    assert_eq!(style_of(&s, id).centre_mark_gap(), arm);
    assert_eq!(style_of(&s, id).centre_mark_extension(), arm);
    let explicit = CentreStyle::INHERIT
        .with_mark_gap(Some(CentreMarkSize::Fixed(arm)))
        .with_mark_extension(Some(CentreMarkSize::Fixed(arm)))
        .with_mark_line_font(Some(false));
    restyle(&mut s, id, explicit);
    assert_eq!(ap(&s, annot), before);
    assert_eq!(extension_reach(&s, id), vec![RADIUS + arm; 4]);
}

/// The driven check: the extension-end snaps move by exactly the stated
/// amounts, and the gap moves where the drawn line starts.
#[test]
fn gap_and_extension_move_the_drawn_lines_and_the_snaps() {
    let (mut s, annot, id) = one();
    restyle(
        &mut s,
        id,
        CentreStyle::INHERIT
            .with_mark_gap(Some(CentreMarkSize::Fixed(20.0)))
            .with_mark_extension(Some(CentreMarkSize::Fixed(2.0))),
    );
    assert_eq!(extension_reach(&s, id), vec![RADIUS + 2.0; 4]);
    let content = ap(&s, annot);
    // The +x extension line starts at arm (5) + gap (20) from the centre.
    assert!(content.contains("325 300 m"), "{content}");
    assert!(content.contains("352 300 l"), "{content}");
    let model = s.dimension_model();
    let kind = model.dimension(id).unwrap().kind.clone();
    assert_eq!(
        kind.centre_mark_extension_lines_drawn(style_of(&s, id)),
        Some(true)
    );
}

/// Would catch: extension lines drawn backwards, or the request silently
/// dropped, when the gap reaches past the rim plus the extension.
#[test]
fn a_gap_past_the_rim_draws_no_extension_lines_and_says_so() {
    let (mut s, annot, id) = one();
    restyle(
        &mut s,
        id,
        CentreStyle::INHERIT.with_mark_gap(Some(CentreMarkSize::TextHeight(6.0))),
    );
    assert!(extension_reach(&s, id).is_empty());
    let kind = s.dimension_model().dimension(id).unwrap().kind.clone();
    assert_eq!(
        kind.centre_mark_extension_lines_drawn(style_of(&s, id)),
        Some(false)
    );
    assert_eq!(ap(&s, annot).matches(" m\n").count(), 2);
    let bare = DimensionKind::CentreMark {
        fit: FitCircle {
            center: CENTRE,
            radius: RADIUS,
            residual: 0.0,
        },
        mark: CentreMarkSpec::default(),
    };
    assert_eq!(
        bare.centre_mark_extension_lines_drawn(style_of(&s, id)),
        None
    );
}

/// Would catch: the line font dashing the cross arms, or one pattern laid
/// across all four lines instead of each fitted on its own.
#[test]
fn line_font_dashes_each_extension_line_and_keeps_the_arms_solid() {
    let (mut s, annot, id) = one();
    restyle(
        &mut s,
        id,
        CentreStyle::INHERIT
            .with_line(PartStroke::FOLLOW.with_dash(DimDash::new(&[6.0, 2.0])))
            .with_mark_line_font(Some(true)),
    );
    let content = ap(&s, annot);
    let lines: Vec<&str> = content.lines().collect();
    let first_arm = lines.iter().position(|l| l.ends_with(" m")).unwrap();
    let dashes: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.ends_with(" d"))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(dashes.len(), 4, "{content}");
    assert!(dashes[0] > first_arm, "arms are dashed: {content}");
    assert!(
        lines[..first_arm].contains(&"q"),
        "no state wrap: {content}"
    );
    // Each extension line is its own subpath, stroked on its own.
    assert_eq!(content.matches("\nS\n").count(), 5, "{content}");
}

#[test]
fn reach_survives_save_with_provenance_and_version_9() {
    let (mut s, _, id) = one();
    let g = GroupStyle {
        centre: CentreStyle::INHERIT.with_mark_extension(Some(CentreMarkSize::TextHeight(0.25))),
        ..GroupStyle::default()
    };
    s.set_group_style(DEFAULT_GROUP_ID, g).unwrap();
    restyle(
        &mut s,
        id,
        CentreStyle::INHERIT
            .with_mark_gap(Some(CentreMarkSize::Fixed(3.0)))
            .with_mark_line_font(Some(true)),
    );
    let back = EditSession::new(Document::from_bytes(save(&s)).unwrap()).dimension_model();
    let group = back.group(DEFAULT_GROUP_ID).unwrap();
    assert_eq!(
        group.style.centre.mark_extension,
        Some(CentreMarkSize::TextHeight(0.25))
    );
    let rec = back.dimension(id).unwrap();
    assert_eq!(rec.style.centre.mark_gap, Some(CentreMarkSize::Fixed(3.0)));
    assert_eq!(rec.style.centre.mark_line_font, Some(true));
    assert_eq!(rec.style.count(), 2);
    let p = style_provenance(group, &rec.style);
    assert_eq!(p.centre_mark_gap, StyleSource::Dimension);
    assert_eq!(p.centre_mark_extension, StyleSource::Group);
    assert_eq!(p.centre_mark_line_font, StyleSource::Dimension);
    assert_eq!(p.each().len(), 26);
    let private = pdfcer_core::dimension::serialize_model(&back);
    assert_eq!(sidecar_version(&private), Some(9));
}

/// `CentreStyle` is `#[non_exhaustive]`: the reach fields are set through
/// the builders and merged by `or`.
#[test]
fn reach_fields_are_built_outside_the_crate() {
    let upper = CentreStyle::INHERIT.with_mark_gap(Some(CentreMarkSize::Fixed(1.0)));
    let lower = CentreStyle::INHERIT
        .with_mark_gap(Some(CentreMarkSize::Fixed(9.0)))
        .with_mark_extension(Some(CentreMarkSize::Fixed(4.0)))
        .with_mark_line_font(Some(true));
    let merged = upper.or(lower);
    assert_eq!(merged.mark_gap, Some(CentreMarkSize::Fixed(1.0)));
    assert_eq!(merged.mark_extension, Some(CentreMarkSize::Fixed(4.0)));
    assert_eq!(merged.mark_line_font, Some(true));
    assert_eq!(merged.count(), 3);
    assert!(mark_with_extensions().reads_style_field("centre-mark-gap"));
    assert!(mark_with_extensions().reads_style_field("centre-mark-line-font"));
}
