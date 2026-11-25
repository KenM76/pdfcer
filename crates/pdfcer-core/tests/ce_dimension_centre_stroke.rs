//! A centre line's and centre mark's own stroke (pdfcer-gui request G334):
//! cascaded per field, never taken from the ce dimension's `dash`, and the
//! centre line's dash fit disclosed.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::annot_author::Color;
use pdfcer_core::dimension::{
    CentreMarkSpec, CentreStyle, DEFAULT_GROUP_ID, DashFit, DimDash, DimensionId, DimensionKind,
    FitCircle, GroupStyle, PartStroke, StyleOverrides, StyleSource, resolve_style, sidecar_version,
    style_provenance,
};
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::vector::{AxisConstraint, Point};
use pdfcer_core::writer::SaveOptions;
use std::path::Path;

fn hello() -> EditSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/hello.pdf");
    EditSession::new(Document::load(&path).expect("load fixture"))
}

fn fit() -> FitCircle {
    FitCircle {
        center: Point::new(300.0, 300.0),
        radius: 50.0,
        residual: 0.0,
    }
}

fn centre_line(length: f64) -> DimensionKind {
    DimensionKind::CentreLine {
        a: Point::new(100.0, 400.0),
        b: Point::new(100.0 + length, 400.0),
    }
}

fn centre_mark() -> DimensionKind {
    DimensionKind::CentreMark {
        fit: fit(),
        mark: CentreMarkSpec::default(),
    }
}

fn circular_with_mark() -> DimensionKind {
    DimensionKind::Circular {
        fit: fit(),
        show_diameter: false,
        show_circle: true,
        leader_angle: 30.0,
        text_distance: None,
        centre_mark: Some(CentreMarkSpec::default()),
    }
}

fn save(s: &EditSession) -> Vec<u8> {
    s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0
}

/// The saved annotation dict and its decoded `/AP /N` content.
fn saved(s: &EditSession, id: ObjId) -> (pdfcer_core::object::Dict, String) {
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
    let content =
        String::from_utf8_lossy(&pdfcer_core::filters::decode_stream(&st.dict, raw).unwrap())
            .into_owned();
    (annot, content)
}

fn ap(s: &EditSession, id: ObjId) -> String {
    saved(s, id).1
}

fn one(kind: DimensionKind) -> (EditSession, ObjId, DimensionId) {
    let mut s = hello();
    let (annot, id) = s.add_dimension(0, DEFAULT_GROUP_ID, kind).unwrap();
    (s, annot, id)
}

fn group_dash(s: &mut EditSession, dash: DimDash) {
    let g = GroupStyle {
        dash: Some(dash),
        ..GroupStyle::default()
    };
    s.set_group_style(DEFAULT_GROUP_ID, g).unwrap();
}

fn restyle(s: &mut EditSession, id: DimensionId, centre: CentreStyle) {
    let style = StyleOverrides {
        centre,
        ..StyleOverrides::default()
    };
    s.set_dimension_style(id, style).unwrap();
}

fn dash_lines(ap: &str) -> Vec<&str> {
    ap.lines().filter(|l| l.ends_with(" d")).collect()
}

/// Would catch: a centre line reading the ce dimension's own `dash` — the
/// sentence the core-api doc used to make. A group that dashes its
/// measurements must not turn its centre lines into hidden lines.
#[test]
fn a_group_dash_does_not_change_a_centre_line() {
    let (mut s, annot, _) = one(centre_line(300.0));
    let before = ap(&s, annot);
    group_dash(&mut s, DimDash::new(&[6.0, 2.0]).unwrap());
    assert_eq!(ap(&s, annot), before);
    assert_eq!(dash_lines(&before).len(), 1, "{before}");
}

#[test]
fn a_centre_line_dash_override_is_drawn_fitted_to_the_line() {
    let (mut s, annot, id) = one(centre_line(100.0));
    let dash = DimDash::new(&[6.0, 2.0]).unwrap();
    restyle(
        &mut s,
        id,
        CentreStyle::INHERIT.with_line(PartStroke::FOLLOW.with_dash(Some(dash))),
    );
    let ap = ap(&s, annot);
    // 100 = 12 cycles of 8 plus a closing 6 at k = 100/102.
    let k = 100.0 / 102.0;
    let want = format!("[{} {}] 0 d", fmt(6.0 * k), fmt(2.0 * k));
    assert_eq!(dash_lines(&ap), vec![want.as_str()], "{ap}");
    let rec = s.dimension_model().dimension(id).cloned().unwrap();
    let group = s
        .dimension_model()
        .group(DEFAULT_GROUP_ID)
        .cloned()
        .unwrap();
    let style = resolve_style(&group, &rec.style);
    assert_eq!(
        rec.kind.centre_line_dash_fit(style),
        Some(DashFit::Stretched)
    );
}

/// How `ContentBuilder` writes a real: the same shortest round-trip form.
fn fmt(v: f64) -> String {
    let mut b = pdfcer_core::writer::content::ContentBuilder::new();
    b.set_line_width(v);
    let s = String::from_utf8(b.into_bytes()).unwrap();
    s.trim().trim_end_matches(" w").to_owned()
}

#[test]
fn a_centre_line_width_and_colour_override_reach_the_stroke_and_c() {
    let (mut s, annot, id) = one(centre_line(300.0));
    let part = PartStroke::FOLLOW
        .with_width(Some(0.35))
        .with_color(Some(Color::Rgb(0.0, 0.0, 1.0)));
    restyle(&mut s, id, CentreStyle::INHERIT.with_line(part));
    let (dict, ap) = saved(&s, annot);
    assert!(ap.contains("0.35 w"), "{ap}");
    assert!(ap.contains("0 0 1 RG"), "{ap}");
    let c = dict.get(b"C").unwrap();
    assert_eq!(
        c,
        &Object::Array(vec![
            Object::Real(0.0),
            Object::Real(0.0),
            Object::Real(1.0)
        ])
    );
}

#[test]
fn a_too_short_centre_line_is_drawn_solid_and_says_so() {
    let (s, annot, id) = one(centre_line(40.0));
    assert!(dash_lines(&ap(&s, annot)).is_empty());
    let rec = s.dimension_model().dimension(id).cloned().unwrap();
    let group = s
        .dimension_model()
        .group(DEFAULT_GROUP_ID)
        .cloned()
        .unwrap();
    let fit = rec
        .kind
        .centre_line_dash_fit(resolve_style(&group, &rec.style));
    assert_eq!(fit, Some(DashFit::SolidTooShort));
    assert_eq!(fit.unwrap().token(), "solid-too-short");
    assert_eq!(
        centre_mark().centre_line_dash_fit(resolve_style(&group, &rec.style)),
        None
    );
}

/// Would catch: the factory mark losing the `q [] 0 d … Q` it was always
/// baked with under a dashed group (a rewrite of every such `/AP`).
#[test]
fn a_solid_mark_under_a_dashed_group_keeps_its_reset() {
    let (mut s, annot, _) = one(centre_mark());
    assert!(dash_lines(&ap(&s, annot)).is_empty());
    group_dash(&mut s, DimDash::new(&[6.0, 2.0]).unwrap());
    let ap = ap(&s, annot);
    assert_eq!(dash_lines(&ap), vec!["[] 0 d"], "{ap}");
    assert!(ap.contains("\nq\n") || ap.starts_with("q\n"), "{ap}");
}

#[test]
fn a_dashed_mark_on_a_circular_strokes_in_its_own_state() {
    let (mut s, annot, id) = one(circular_with_mark());
    let before = ap(&s, annot);
    let part = PartStroke::FOLLOW
        .with_dash(DimDash::new(&[2.0, 1.0]))
        .with_color(Some(Color::Rgb(1.0, 0.0, 0.0)));
    restyle(&mut s, id, CentreStyle::INHERIT.with_mark(part));
    let ap = ap(&s, annot);
    assert_ne!(ap, before);
    let lines: Vec<&str> = ap.lines().collect();
    let d = lines.iter().position(|l| *l == "[2 1] 0 d").expect("dash");
    let q = lines[..d].iter().rposition(|l| *l == "q").expect("q");
    assert!(lines[q..d].contains(&"1 0 0 RG"), "{ap}");
    // The state closes before the label, which keeps the lines' fill.
    let close = lines[d..].iter().position(|l| *l == "Q").expect("Q") + d;
    assert!(lines[close..].contains(&"BT"), "{ap}");
}

#[test]
fn centre_styles_survive_save_with_provenance_and_version_8() {
    let (mut s, _, id) = one(centre_line(300.0));
    let g = GroupStyle {
        centre: CentreStyle::INHERIT.with_mark(PartStroke::FOLLOW.with_width(Some(0.25))),
        ..GroupStyle::default()
    };
    s.set_group_style(DEFAULT_GROUP_ID, g).unwrap();
    let dash = DimDash::new(&[12.0, 3.0]);
    restyle(
        &mut s,
        id,
        CentreStyle::INHERIT.with_line(PartStroke::FOLLOW.with_dash(dash)),
    );
    let back = EditSession::new(Document::from_bytes(save(&s)).unwrap()).dimension_model();
    let group = back.group(DEFAULT_GROUP_ID).unwrap();
    assert_eq!(group.style.centre.mark.width, Some(0.25));
    let rec = back.dimension(id).unwrap();
    assert_eq!(rec.style.centre.line.dash, dash);
    assert_eq!(rec.style.count(), 1);
    let p = style_provenance(group, &rec.style);
    assert_eq!(p.centre_line_dash, StyleSource::Dimension);
    assert_eq!(p.centre_mark_width, StyleSource::Group);
    assert_eq!(p.centre_line_width, StyleSource::Factory);
    assert_eq!(p.each().len(), 26);
    let private = pdfcer_core::dimension::serialize_model(&back);
    assert_eq!(sidecar_version(&private), Some(8));
}

#[test]
fn each_kind_reads_only_its_own_style_fields() {
    let linear = DimensionKind::Linear {
        a: Point::new(0.0, 0.0),
        b: Point::new(10.0, 0.0),
        constraint: AxisConstraint::Horizontal,
        offset: 0.0,
        text_along: 0.0,
        extension_gap: [None; 2],
    };
    assert!(linear.reads_style_field("dash"));
    assert!(!linear.reads_style_field("circle-dash"));
    assert!(!linear.reads_style_field("centre-line-dash"));
    let line = centre_line(10.0);
    assert!(line.reads_style_field("centre-line-width"));
    assert!(!line.reads_style_field("tolerance"));
    let mark = centre_mark();
    assert!(mark.reads_style_field("centre-mark-color"));
    assert!(mark.reads_style_field("text-height"));
    assert!(!mark.reads_style_field("centre-line-dash"));
    let circ = circular_with_mark();
    assert!(circ.reads_style_field("circle-width"));
    assert!(circ.reads_style_field("centre-mark-dash"));
    assert!(!circ.reads_style_field("no-such-field"));
}

/// `CentreStyle` and `DashFit` are `#[non_exhaustive]`: a consumer builds
/// the one through `INHERIT` and the builders and matches the other with a
/// wildcard.
#[test]
fn centre_styles_are_built_outside_the_crate() {
    let s = CentreStyle::INHERIT
        .with_line(PartStroke::FOLLOW.with_width(Some(1.0)))
        .with_mark(PartStroke::FOLLOW.with_color(Some(Color::Gray(0.5))));
    assert_eq!(s.count(), 2);
    assert!(CentreStyle::default().is_inherit());
    let lower = CentreStyle::INHERIT.with_line(PartStroke::FOLLOW.with_dash(Some(DimDash::SOLID)));
    let merged = s.or(lower);
    assert_eq!(merged.line.dash, Some(DimDash::SOLID));
    assert_eq!(merged.line.width, Some(1.0));
    let token = match DashFit::Nominal {
        DashFit::Nominal => "n",
        _ => "other",
    };
    assert_eq!(token, "n");
}
