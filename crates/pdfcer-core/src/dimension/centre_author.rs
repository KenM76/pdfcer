//! The `/AP` baker for centre lines and centre marks.
//!
//! Neither measures anything, so neither carries `/IT`, `/Measure` or a
//! label. ISO 32000-1 §12.5.6.7 Table 175 defines only `LineArrow` and
//! `LineDimension` as line intents, so a centre line is a plain `/Line`; a
//! centre mark is two or six disjoint strokes, which is what an `/Ink`
//! annotation's `/InkList` (§12.5.6.13 Table 182) expresses. pdfcer's
//! `/PieceInfo` sidecar is what reads them back as centre geometry.

use crate::object::{Dict, Name, Object};
use crate::textstring::encode_text_string;
use crate::vector::Point;
use crate::writer::content::{ContentBuilder, Paint};

use super::author::{
    AuthoredDimension, BoundsAcc, DimensionStyle, ap_form_dict, begin_strokes, rect_array,
};
use super::centre::{CentreMarkSpec, fitted_centre_dash, mark_segments};
use super::fit::FitCircle;
use super::group::DimensionKind;

/// `/Contents` of a centre line: its §12.5.2 alternate description.
const CENTRE_LINE_CONTENTS: &str = "Centre line";
/// `/Contents` of a centre mark.
const CENTRE_MARK_CONTENTS: &str = "Centre mark";

/// Bake a [`DimensionKind::CentreLine`] or [`DimensionKind::CentreMark`].
/// Any other kind bakes as an empty `/Line` (callers route only centre
/// geometry here).
pub(super) fn author_centre_geometry(
    kind: &DimensionKind,
    style: DimensionStyle,
) -> AuthoredDimension {
    let mut b = ContentBuilder::new();
    let mut bounds = BoundsAcc::new();
    begin_strokes(&mut b, style);
    let mut annot = Dict::new();
    annot.insert(Name::from(b"Type"), Object::Name(Name::from(b"Annot")));
    let (label, at) = match *kind {
        DimensionKind::CentreMark { fit, mark } => {
            let strokes = draw_mark(&mut b, &mut bounds, fit, mark, style);
            annot.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"Ink")));
            let ink = strokes
                .iter()
                .map(|&(p, q)| Object::Array([p.x, p.y, q.x, q.y].map(Object::Real).to_vec()))
                .collect();
            annot.insert(Name::from(b"InkList"), Object::Array(ink));
            (CENTRE_MARK_CONTENTS, fit.center)
        }
        DimensionKind::CentreLine { a, b: end } => {
            draw_centre_line(&mut b, &mut bounds, a, end, style);
            annot.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"Line")));
            let l = [a.x, a.y, end.x, end.y].map(Object::Real).to_vec();
            annot.insert(Name::from(b"L"), Object::Array(l));
            (CENTRE_LINE_CONTENTS, a.midpoint(end))
        }
        _ => {
            annot.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"Line")));
            ("", Point::new(0.0, 0.0))
        }
    };
    let rect = bounds.into_rect();
    annot.insert(Name::from(b"Rect"), rect_array(rect));
    annot.insert(Name::from(b"C"), style.color.to_array());
    if style.opacity < 1.0 {
        annot.insert(Name::from(b"CA"), Object::Real(style.opacity));
    }
    annot.insert(
        Name::from(b"Contents"),
        Object::String(encode_text_string(label)),
    );
    AuthoredDimension {
        annot,
        ap_dict: ap_form_dict(rect),
        ap_content: b.into_bytes(),
        rect,
        label: label.to_owned(),
        label_overridden: false,
        label_quad: [at; 4],
    }
}

/// Stroke a centre line with the long-short-long dash fitted to its length,
/// solid when it is too short for one cycle.
fn draw_centre_line(
    b: &mut ContentBuilder,
    bounds: &mut BoundsAcc,
    p: Point,
    q: Point,
    style: DimensionStyle,
) {
    let length = (q.x - p.x).hypot(q.y - p.y);
    if let Some(dash) = fitted_centre_dash(style.centre_line_dash(), length) {
        b.set_dash(&dash, 0.0);
    }
    b.move_to(p.x, p.y);
    b.line_to(q.x, q.y);
    b.paint(Paint::Stroke);
    bounds.add(p);
    bounds.add(q);
}

/// Stroke a centre mark, always solid: inside a dashed circular ce dimension
/// the mark is wrapped in `q [] 0 d … Q`. Returns the strokes drawn.
pub(super) fn draw_mark(
    b: &mut ContentBuilder,
    bounds: &mut BoundsAcc,
    fit: FitCircle,
    mark: CentreMarkSpec,
    style: DimensionStyle,
) -> Vec<(Point, Point)> {
    let strokes = mark_segments(fit, mark, style.centre_mark_arm());
    let dashed = !style.dash.is_solid();
    if dashed {
        b.save_state();
        b.set_dash(&[], 0.0);
    }
    for &(p, q) in &strokes {
        b.move_to(p.x, p.y);
        b.line_to(q.x, q.y);
        bounds.add(p);
        bounds.add(q);
    }
    b.paint(Paint::Stroke);
    if dashed {
        b.restore_state();
    }
    strokes
}
