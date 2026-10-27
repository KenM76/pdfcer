//! Painting markup annotations that carry no `/AP` from their own geometry
//! (decision 203: R43 narrowed to "derived, not invented").
//!
//! The look comes from [`pdfcer_core::annot_author`], the same baker an
//! authored annotation's `/AP` comes from, fed by
//! [`pdfcer_core::annot_author::spec_from_dict`]. It is painted, never
//! stored. Anything the file does not state — a missing `/C`, a 2.0
//! `/Path`-only shape, a pdf dimension — is declined and noted, not guessed.

use pdfcer_core::annot::Annotation;
use pdfcer_core::annot_author::{self, ColorEntry, MarkupSpec};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{Dict, Object};
use pdfcer_core::view::DocumentView;
use tiny_skia::Transform;

use super::{
    fit_matrix, fixed_placement_ctm, read_matrix, read_rect_numbers, transformed_appearance_box,
};
use crate::canvas::Canvas;
use crate::font::{FontEnvironment, RenderPolicy};
use crate::gstate::GraphicsState;
use crate::interpret::Diagnostics;

#[path = "free_text.rs"]
mod free_text;
pub use free_text::FreeTextC;

/// What the renderer does with a markup annotation (Ink, Highlight,
/// Underline, StrikeOut, Squiggly, Square, Circle, Polygon, PolyLine, Line,
/// FreeText) that has no `/AP`.
///
/// ISO 32000-1 §12.5.2 lets a reader "provide their own appearances"; the
/// geometry keys of §12.5.6.7–§12.5.6.13 fully determine these subtypes'
/// look. A `/Line` carrying `/Cap`, `/LL` or `/IT /LineDimension` is a pdf
/// dimension and is never drawn this way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum MarkupWithoutAp {
    /// Draw it from its geometry and colours, as Acrobat does. The default.
    #[default]
    FromGeometry,
    /// Leave it blank (the file stores no drawing).
    Blank,
}

/// Why an in-class annotation was not drawn.
enum Declined {
    /// The geometry is malformed or only in a 2.0 `/Path`.
    Geometry(String),
    /// No device colour to draw with.
    NoColour,
    /// A `/Line` that is a measurement — a pdf dimension.
    Dimension,
}

/// The subtypes this module draws.
fn in_class(subtype: &str) -> bool {
    matches!(
        subtype,
        "Ink"
            | "Highlight"
            | "Underline"
            | "StrikeOut"
            | "Squiggly"
            | "Square"
            | "Circle"
            | "Polygon"
            | "PolyLine"
            | "Line"
            | "FreeText"
    )
}

/// §12.5.6.7 Table 175: `/Cap true`, `/LL` and `/IT /LineDimension` make a
/// line a dimension.
fn is_dimension_line(doc: &DocumentView<'_>, dict: &Dict) -> bool {
    let cap = matches!(
        dict.get(b"Cap").map(|o| doc.resolve(o)),
        Some(Object::Boolean(true))
    );
    let it_dim = matches!(
        dict.get(b"IT").map(|o| doc.resolve(o)),
        Some(Object::Name(n)) if n.as_bytes() == b"LineDimension"
    );
    cap || it_dim || dict.get(b"LL").is_some()
}

fn has_colour(doc: &DocumentView<'_>, dict: &Dict, key: &[u8]) -> bool {
    matches!(
        annot_author::color_entry(doc, dict, key),
        ColorEntry::Device(_)
    )
}

/// The spec to bake, or why not.
fn spec_for(doc: &DocumentView<'_>, dict: &Dict, subtype: &str) -> Result<MarkupSpec, Declined> {
    if subtype == "Line" && is_dimension_line(doc, dict) {
        return Err(Declined::Dimension);
    }
    let closed = matches!(subtype, "Square" | "Circle" | "Polygon");
    let coloured = if closed {
        has_colour(doc, dict, b"C") || has_colour(doc, dict, b"IC")
    } else {
        has_colour(doc, dict, b"C")
    };
    if !coloured {
        return Err(Declined::NoColour);
    }
    let mut spec =
        annot_author::spec_from_dict(doc, dict).map_err(|e| Declined::Geometry(e.to_string()))?;
    // §12.5.6.8: the shape sits inside /Rect less /RD. `spec_from_dict`
    // insets only under a border effect, which is the authoring round trip.
    if let MarkupSpec::Square {
        rect,
        border_effect: None,
        ..
    }
    | MarkupSpec::Circle { rect, .. } = &mut spec
        && let Some(inner) = annot_author::rect_inside_differences(doc, dict)
    {
        *rect = inner;
    }
    Ok(spec)
}

/// A baked look: the `/AP` `/N` form dict, its content, the rect it is
/// placed at, and any disclosures it owes.
struct Look {
    ap_dict: Dict,
    ap_content: Vec<u8>,
    rect: pdfcer_core::page_tree::Rect,
    notes: Vec<String>,
}

/// Bake `dict`'s look, or say why it was declined.
fn look_for(
    doc: &DocumentView<'_>,
    dict: &Dict,
    subtype: &str,
    policy: RenderPolicy,
) -> Result<Look, String> {
    if subtype == "FreeText" {
        let baked = free_text::bake(doc, dict, policy.free_text_c)?;
        return Ok(Look {
            ap_dict: baked.annot.ap_dict,
            ap_content: baked.annot.ap_content,
            rect: baked.annot.rect,
            notes: baked.notes,
        });
    }
    let spec = spec_for(doc, dict, subtype).map_err(|why| match why {
        Declined::Geometry(e) => e,
        Declined::NoColour => "it states no colour to draw with (/C)".to_owned(),
        Declined::Dimension => "it is a pdf dimension (/Cap, /LL or /IT /LineDimension)".to_owned(),
    })?;
    let opts = annot_author::appearance_options_from_dict(doc, dict);
    let baked = annot_author::build_appearance_opts(&spec, &opts);
    Ok(Look {
        ap_dict: baked.ap_dict,
        ap_content: baked.ap_content,
        rect: baked.rect,
        notes: Vec::new(),
    })
}

/// Draw markup `annot` from its own geometry. `false` when nothing was drawn
/// (out of class, setting off, declined — the last with a note).
#[allow(clippy::too_many_arguments)] // every argument is placement input.
pub(super) fn paint_from_geometry(
    doc: &DocumentView<'_>,
    base_ctm: Transform,
    fonts: &FontEnvironment,
    annot: &Annotation,
    diag: &mut Diagnostics,
    canvas: &mut Canvas<'_>,
    cancel: Option<&crate::cancel::RenderCancel>,
    policy: RenderPolicy,
) -> bool {
    let subtype = annot.subtype_label();
    if !in_class(&subtype) || policy.markup_without_ap != MarkupWithoutAp::FromGeometry {
        return false;
    }
    let Some(dict) = annot
        .id
        .and_then(|id| doc.value(id))
        .and_then(Object::as_dict)
    else {
        return false;
    };
    let look = match look_for(doc, dict, &subtype, policy) {
        Ok(look) => look,
        Err(reason) => {
            diag.note_annotation(&format!(
                "/{subtype} has no /AP and was left blank: {reason}"
            ));
            return false;
        }
    };
    if !paint_baked(
        doc, base_ctm, fonts, annot, &look, diag, canvas, cancel, policy,
    ) {
        return false;
    }
    *diag
        .annotations_geometry_drawn
        .entry(subtype.clone())
        .or_insert(0) += 1;
    diag.note_annotation(&format!(
        "/{subtype} has no /AP - drawn from its own geometry and colours (12.5.2 lets the reader \
         provide the appearance); nothing was written to the file{}",
        look.notes
            .iter()
            .map(|n| format!("; {n}"))
            .collect::<String>()
    ));
    true
}

/// Place `baked` against its own `/Rect` (the baker's geometry is in default
/// user space, so the mapping is the identity) and paint it at the file's
/// `/CA`, composited as one object as `paint_appearance` does.
#[allow(clippy::too_many_arguments)] // every argument is placement input.
fn paint_baked(
    doc: &DocumentView<'_>,
    base_ctm: Transform,
    fonts: &FontEnvironment,
    annot: &Annotation,
    baked: &Look,
    diag: &mut Diagnostics,
    canvas: &mut Canvas<'_>,
    cancel: Option<&crate::cancel::RenderCancel>,
    policy: RenderPolicy,
) -> bool {
    let rect = baked.rect;
    let Some(bbox) = read_rect_numbers(doc, &baked.ap_dict, b"BBox") else {
        return false;
    };
    let Some(tbox) = transformed_appearance_box(bbox, read_matrix(doc, &baked.ap_dict)) else {
        return false;
    };
    let placement = fit_matrix(tbox, rect).post_concat(fixed_placement_ctm(
        base_ctm,
        annot,
        rect,
        policy.view_magnification,
    ));
    let Ok(content) = pdfcer_core::content::ContentStream::parse(baked.ap_content.clone()) else {
        return false;
    };
    let resources = baked
        .ap_dict
        .get(b"Resources")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_dict)
        .cloned()
        .unwrap_or_default();
    let run = |c: &mut Canvas<'_>| {
        crate::interpret::run_on(
            doc,
            &content,
            &resources,
            fonts,
            GraphicsState::default_with_ctm(placement),
            c,
            cancel,
            policy,
            crate::compositor::BlendSpace::Additive,
        )
    };
    let alpha = annot.constant_alpha.unwrap_or(1.0);
    let sub = if alpha >= 1.0 {
        run(canvas)
    } else {
        #[allow(clippy::cast_possible_truncation)] // an opacity in [0, 1].
        let paint = crate::canvas::LayerPaint {
            opacity: alpha as f32,
            blend: tiny_skia::BlendMode::SourceOver,
            nonseparable: None,
        };
        match canvas.layer(paint, run) {
            Some(sub) => sub,
            None => return false,
        }
    };
    diag.merge(sub);
    true
}
