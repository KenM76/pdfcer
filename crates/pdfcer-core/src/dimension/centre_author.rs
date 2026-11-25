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
    stroke_paint,
};
use super::centre::{CentreMarkSpec, mark_segments};
use super::centre_style::fit_dash;
use super::dash::DimDash;
use super::fit::FitCircle;
use super::group::DimensionKind;
use super::part_stroke::ResolvedStroke;

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
    let own = match kind {
        DimensionKind::CentreMark { .. } => style.centre_mark_stroke(),
        _ => style.centre_line_stroke(),
    };
    let state = ResolvedStroke::new(DimDash::SOLID, own.width, own.color);
    let style_drawn = DimensionStyle {
        line_width: own.width,
        color: own.color,
        ..style
    };
    begin_strokes(&mut b, style_drawn);
    let mut annot = Dict::new();
    annot.insert(Name::from(b"Type"), Object::Name(Name::from(b"Annot")));
    let (label, at) = match *kind {
        DimensionKind::CentreMark { fit, mark } => {
            let strokes = draw_mark(&mut b, &mut bounds, (fit, mark), style, state);
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
    annot.insert(Name::from(b"C"), own.color.to_array());
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

/// Stroke a centre line in [`DimensionStyle::centre_line_stroke`], its dash
/// fitted to its length, solid when it is too short for one cycle. The width
/// and colour were set by [`begin_strokes`].
fn draw_centre_line(
    b: &mut ContentBuilder,
    bounds: &mut BoundsAcc,
    p: Point,
    q: Point,
    style: DimensionStyle,
) {
    let length = (q.x - p.x).hypot(q.y - p.y);
    let (dash, _) = fit_dash(style.centre_line_stroke().dash, length);
    if !dash.is_empty() {
        b.set_dash(&dash, 0.0);
    }
    b.move_to(p.x, p.y);
    b.line_to(q.x, q.y);
    b.paint(Paint::Stroke);
    bounds.add(p);
    bounds.add(q);
}

/// Stroke a centre mark in [`DimensionStyle::centre_mark_stroke`]. `state`
/// is the stroke already in the graphics state: the ce dimension's lines
/// inside a radius/diameter ce dimension, the mark's own width and colour
/// for a standalone mark. Fields that differ are set inside `q … Q`; a solid
/// mark under dashed lines always resets the dash there. Returns the
/// strokes drawn.
pub(super) fn draw_mark(
    b: &mut ContentBuilder,
    bounds: &mut BoundsAcc,
    (fit, mark): (FitCircle, CentreMarkSpec),
    style: DimensionStyle,
    state: ResolvedStroke,
) -> Vec<(Point, Point)> {
    let strokes = mark_segments(fit, mark, style);
    let part = style.centre_mark_stroke();
    let reset_dash = part.dash != state.dash || (part.dash.is_solid() && !style.dash.is_solid());
    let line_font = style.centre_mark_line_font() && strokes.len() > 2;
    let wrap = reset_dash || line_font || part.width != state.width || part.color != state.color;
    if wrap {
        b.save_state();
        if part.color != state.color {
            stroke_paint(part.color).apply_stroke(b);
        }
        if part.width != state.width {
            b.set_line_width(part.width);
        }
        if reset_dash {
            b.set_dash(part.dash.pattern(), 0.0);
        }
    }
    let (arms, extensions) = strokes.split_at(if line_font { 2 } else { strokes.len() });
    for &(p, q) in arms {
        b.move_to(p.x, p.y);
        b.line_to(q.x, q.y);
        bounds.add(p);
        bounds.add(q);
    }
    b.paint(Paint::Stroke);
    stroke_in_line_font(b, bounds, extensions, style);
    if wrap {
        b.restore_state();
    }
    strokes
}

/// Stroke each extension line on its own, in the centre-line dash fitted to
/// its length (solid when too short). Nothing for an empty slice.
fn stroke_in_line_font(
    b: &mut ContentBuilder,
    bounds: &mut BoundsAcc,
    extensions: &[(Point, Point)],
    style: DimensionStyle,
) {
    let nominal = style.centre_line_stroke().dash;
    for &(p, q) in extensions {
        let (dash, _) = fit_dash(nominal, (q.x - p.x).hypot(q.y - p.y));
        b.set_dash(&dash, 0.0);
        b.move_to(p.x, p.y);
        b.line_to(q.x, q.y);
        b.paint(Paint::Stroke);
        bounds.add(p);
        bounds.add(q);
    }
}
