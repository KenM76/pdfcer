//! Painting form-field widgets that carry no `/AP` (§12.7.3.3: the reader
//! "shall construct an appearance stream dynamically at viewing time").
//! The appearance comes from [`pdfcer_core::widget_draw`], the same layout a
//! redraw would write; here it is only painted, never stored.

use pdfcer_core::annot::Annotation;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::Object;
use pdfcer_core::view::DocumentView;
use pdfcer_core::widget_draw::WidgetAppearances;
use tiny_skia::Transform;

use super::{
    fit_matrix, fixed_placement_ctm, read_matrix, read_rect_numbers, transformed_appearance_box,
};
use crate::canvas::Canvas;
use crate::font::{FontEnvironment, RenderPolicy};
use crate::gstate::GraphicsState;
use crate::interpret::Diagnostics;

/// What the renderer does with a form-field widget that has no `/AP`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum WidgetsWithoutAp {
    /// Draw it from its field data, as Acrobat does. The default.
    #[default]
    Draw,
    /// Leave it blank (the file stores no drawing).
    Blank,
}

/// The page's form, parsed on first use so a page with no AP-less widget
/// never pays for it.
#[derive(Default)]
pub(super) struct FormCache(Option<Option<WidgetAppearances>>);

impl FormCache {
    fn get(&mut self, doc: &DocumentView<'_>) -> Option<&WidgetAppearances> {
        self.0
            .get_or_insert_with(|| WidgetAppearances::read(doc))
            .as_ref()
    }
}

/// Draw widget `annot` from its field data. `false` when there is nothing
/// to draw (not a widget, no field, no `/Rect`, a signature field).
#[allow(clippy::too_many_arguments)] // every argument is placement input.
pub(super) fn paint_synthesized_widget(
    doc: &DocumentView<'_>,
    base_ctm: Transform,
    fonts: &FontEnvironment,
    annot: &Annotation,
    form: &mut FormCache,
    diag: &mut Diagnostics,
    canvas: &mut Canvas<'_>,
    cancel: Option<&crate::cancel::RenderCancel>,
    policy: RenderPolicy,
) -> bool {
    if !annot.is_widget() || policy.widgets_without_ap != WidgetsWithoutAp::Draw {
        return false;
    }
    let (Some(id), Some(rect)) = (annot.id, annot.rect) else {
        return false;
    };
    let Some(drawn) = form.get(doc).and_then(|f| f.appearance(doc, id)) else {
        return false;
    };
    let Some(bbox) = read_rect_numbers(doc, &drawn.ap_dict, b"BBox") else {
        return false;
    };
    let Some(tbox) = transformed_appearance_box(bbox, read_matrix(doc, &drawn.ap_dict)) else {
        return false;
    };
    let placement = fit_matrix(tbox, rect).post_concat(fixed_placement_ctm(
        base_ctm,
        annot,
        rect,
        policy.view_magnification,
    ));
    let Ok(content) = pdfcer_core::content::ContentStream::parse(drawn.content) else {
        return false;
    };
    let resources = drawn
        .ap_dict
        .get(b"Resources")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_dict)
        .cloned()
        .unwrap_or_default();
    let sub = crate::interpret::run_on(
        doc,
        &content,
        &resources,
        fonts,
        GraphicsState::default_with_ctm(placement),
        canvas,
        cancel,
        policy,
        crate::compositor::BlendSpace::Additive,
    );
    diag.merge(sub);
    diag.note_annotation(&format!(
        "widget {id} has no /AP - drawn as a {kind:?} field from its own value, /DA and /MK \
         (12.7.3.3 puts that on the reader); nothing was written to the file",
        kind = drawn.kind,
    ));
    true
}
