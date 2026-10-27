//! Painting markup annotations that carry no `/AP` from their own geometry
//! (decision 203: R43 narrowed to "derived, not invented").
//!
//! The look comes from [`pdfcer_core::annot_author::derive_appearance`], the
//! same derivation the opt-in bake verb writes as `/AP`. It is painted, never
//! stored. Anything the file does not state — a missing `/C`, a 2.0
//! `/Path`-only shape, a pdf dimension — is declined and noted, not guessed.

use pdfcer_core::annot::Annotation;
use pdfcer_core::annot_author::{self, DERIVED_SUBTYPES, DerivedAppearance};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::Object;
use pdfcer_core::view::DocumentView;
use tiny_skia::Transform;

use super::{
    fit_matrix, fixed_placement_ctm, read_matrix, read_rect_numbers, transformed_appearance_box,
};
use crate::canvas::Canvas;
use crate::font::{FontEnvironment, RenderPolicy};
use crate::gstate::GraphicsState;
use crate::interpret::Diagnostics;

pub use pdfcer_core::annot_author::FreeTextC;

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
    if !DERIVED_SUBTYPES.contains(&subtype.as_str())
        || policy.markup_without_ap != MarkupWithoutAp::FromGeometry
    {
        return false;
    }
    let Some(dict) = annot
        .id
        .and_then(|id| doc.value(id))
        .and_then(Object::as_dict)
    else {
        return false;
    };
    let look = match annot_author::derive_appearance(doc, dict, policy.free_text_c) {
        Ok(look) => look,
        Err(annot_author::NotDerived { reason, .. }) => {
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

/// Place `baked` against the annotation's `/Rect` exactly as the bake verb
/// writes it ([`DerivedAppearance::place`]) and paint it at the file's
/// `/CA`, composited as one object as `paint_appearance` does.
#[allow(clippy::too_many_arguments)] // every argument is placement input.
fn paint_baked(
    doc: &DocumentView<'_>,
    base_ctm: Transform,
    fonts: &FontEnvironment,
    annot: &Annotation,
    baked: &DerivedAppearance,
    diag: &mut Diagnostics,
    canvas: &mut Canvas<'_>,
    cancel: Option<&crate::cancel::RenderCancel>,
    policy: RenderPolicy,
) -> bool {
    let placed = baked.place(annot.rect);
    let rect = placed.rect;
    let Some(bbox) = read_rect_numbers(doc, &placed.ap_dict, b"BBox") else {
        return false;
    };
    let Some(tbox) = transformed_appearance_box(bbox, read_matrix(doc, &placed.ap_dict)) else {
        return false;
    };
    let placement = fit_matrix(tbox, rect).post_concat(fixed_placement_ctm(
        base_ctm,
        annot,
        rect,
        policy.view_magnification,
    ));
    // §8.10.1: a form's /BBox clips it. The saved /AP gets that clip from
    // `do_form`; this path must apply it too, or a stroke touching the box
    // edge bleeds past it here and not once baked.
    let [x0, y0, x1, y1] = bbox;
    let mut clipped = format!("{x0} {y0} {} {} re W n\n", x1 - x0, y1 - y0).into_bytes();
    clipped.extend_from_slice(&baked.ap_content);
    let Ok(content) = pdfcer_core::content::ContentStream::parse(clipped) else {
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
