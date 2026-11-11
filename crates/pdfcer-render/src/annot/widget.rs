//! Painting form-field widgets from their field data (§12.7.3.3: the reader
//! "shall construct an appearance stream dynamically at viewing time"), and
//! the editor's fillable-field highlight.
//! The appearance comes from [`pdfcer_core::widget_draw`], the same layout a
//! redraw would write; here it is only painted, never stored.

use pdfcer_core::annot::Annotation;
use pdfcer_core::forms::{ButtonKind, FieldType};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::view::DocumentView;
use pdfcer_core::widget_draw::{SynthesizedAppearance, SynthesizedKind, WidgetAppearances};
use tiny_skia::{BlendMode, FillRule, PathBuilder, Stroke, Transform};

use super::{
    fit_matrix, fixed_placement_ctm, read_matrix, read_rect_numbers, transformed_appearance_box,
};
use crate::canvas::{BrushSpec, Canvas, ClipRef};
use crate::font::{FontEnvironment, RenderPolicy};
use crate::gstate::{GraphicsState, Rgb};
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

/// What the renderer does with a text or choice widget's stored `/AP` when
/// the form's `/NeedAppearances` is true.
///
/// ISO 32000-1 §12.7.2 Table 218: `/NeedAppearances` "specif\[ies\] whether to
/// construct appearance streams and appearance dictionaries for all widget
/// annotations in the document"; §12.7.3.3 has the reader construct a
/// variable-text appearance at viewing time. The redrawn appearance is the
/// one [`WidgetsWithoutAp::Draw`] uses, painted and never stored.
/// Buttons keep their stored appearances (their states are named art, not a
/// layout of the value) and signature fields are never redrawn.
///
/// # Examples
///
/// ```
/// use pdfcer_render::{NeedAppearances, RenderOptions};
///
/// let options = RenderOptions::default().with_need_appearances(NeedAppearances::AsStored);
/// assert_eq!(options.need_appearances, NeedAppearances::AsStored);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum NeedAppearances {
    /// Draw text and choice widgets from `/V`, as Acrobat does. The default.
    #[default]
    Honour,
    /// Paint the stored `/AP` regardless of the flag.
    AsStored,
}

/// The tint an editor paints behind fillable form fields, and the border it
/// puts on required ones.
///
/// Editor chrome, not document content: drawn only when asked for
/// ([`crate::RenderOptions::with_field_highlight`]), never on a print
/// render, never written. It goes behind each visible widget of a text,
/// choice, check-box, radio or signature field that is not read-only
/// (§12.7.3.1 Table 221 bit 1); push buttons get none. A required field
/// (Table 221 bit 2) also gets a one-point border just inside its `/Rect`,
/// painted over the widget.
///
/// # Examples
///
/// ```
/// use pdfcer_render::{FieldHighlight, RenderOptions};
///
/// let tint = FieldHighlight::new([204, 215, 255, 160], [255, 0, 0, 255]);
/// let options = RenderOptions::default().with_field_highlight(Some(tint));
/// assert_eq!(options.field_highlight, Some(tint));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct FieldHighlight {
    /// The tint, RGBA, straight (not premultiplied) alpha.
    pub fill: [u8; 4],
    /// The required-field border, RGBA.
    pub required_border: [u8; 4],
}

impl FieldHighlight {
    /// A highlight from its two colours.
    #[must_use]
    pub const fn new(fill: [u8; 4], required_border: [u8; 4]) -> Self {
        Self {
            fill,
            required_border,
        }
    }
}

impl Default for FieldHighlight {
    /// A translucent light blue tint and a red border.
    fn default() -> Self {
        Self::new([204, 215, 255, 160], [255, 0, 0, 255])
    }
}

/// The page's form and its `/NeedAppearances` flag, read on first use so a
/// page with no widget never pays for them.
#[derive(Default)]
pub(super) struct FormCache {
    form: Option<Option<WidgetAppearances>>,
    need_appearances: Option<bool>,
}

impl FormCache {
    fn get(&mut self, doc: &DocumentView<'_>) -> Option<&WidgetAppearances> {
        self.form
            .get_or_insert_with(|| WidgetAppearances::read(doc))
            .as_ref()
    }

    fn need_appearances(&mut self, doc: &DocumentView<'_>) -> bool {
        *self
            .need_appearances
            .get_or_insert_with(|| pdfcer_core::annot::need_appearances(doc))
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
    let Some(id) = annot.id else {
        return false;
    };
    let Some(drawn) = form.get(doc).and_then(|f| f.appearance(doc, id)) else {
        return false;
    };
    let kind = drawn.kind;
    if !paint_built(
        doc, base_ctm, fonts, annot, drawn, diag, canvas, cancel, policy,
    ) {
        return false;
    }
    diag.note_annotation(&format!(
        "widget {id} has no /AP - drawn as a {kind:?} field from its own value, /DA and /MK \
         (12.7.3.3 puts that on the reader); nothing was written to the file",
    ));
    true
}

/// Draw a text or choice widget that has an `/AP` from its value instead,
/// because the form sets `/NeedAppearances` and the render honours it.
/// `false` leaves the stored appearance to be painted.
#[allow(clippy::too_many_arguments)] // every argument is placement input.
pub(super) fn paint_for_need_appearances(
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
    if !annot.is_widget()
        || policy.need_appearances != NeedAppearances::Honour
        || !form.need_appearances(doc)
    {
        return false;
    }
    let Some(id) = annot.id else {
        return false;
    };
    let Some(drawn) = form
        .get(doc)
        .and_then(|f| f.appearance(doc, id))
        .filter(|d| matches!(d.kind, SynthesizedKind::Text | SynthesizedKind::Choice))
    else {
        return false;
    };
    let kind = drawn.kind;
    if !paint_built(
        doc, base_ctm, fonts, annot, drawn, diag, canvas, cancel, policy,
    ) {
        return false;
    }
    diag.note_annotation(&format!(
        "widget {id} drawn as a {kind:?} field from its value because /NeedAppearances is true \
         (12.7.2); its stored /AP was not used and nothing was written to the file",
    ));
    true
}

/// Place and paint a built appearance on `annot`'s `/Rect` (§12.5.5).
#[allow(clippy::too_many_arguments)] // every argument is placement input.
fn paint_built(
    doc: &DocumentView<'_>,
    base_ctm: Transform,
    fonts: &FontEnvironment,
    annot: &Annotation,
    drawn: SynthesizedAppearance,
    diag: &mut Diagnostics,
    canvas: &mut Canvas<'_>,
    cancel: Option<&crate::cancel::RenderCancel>,
    policy: RenderPolicy,
) -> bool {
    let Some(rect) = annot.rect else {
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
    true
}

/// Whether widget `id` gets the highlight, and whether it is required.
fn highlight_of(form: &WidgetAppearances, id: ObjId) -> Option<bool> {
    let field = form.field(id)?;
    if field.flags.read_only() {
        return None;
    }
    let fillable = match field.field_type? {
        FieldType::Text | FieldType::Choice | FieldType::Signature => true,
        FieldType::Button => field.button_kind != Some(ButtonKind::Push),
    };
    fillable.then(|| field.flags.required())
}

/// Paint the highlight tint behind `annot`, before the widget itself.
/// Returns the border colour still owed over it, for a required field.
pub(super) fn paint_highlight_fill(
    doc: &DocumentView<'_>,
    base_ctm: Transform,
    annot: &Annotation,
    form: &mut FormCache,
    canvas: &mut Canvas<'_>,
    policy: RenderPolicy,
) -> Option<[u8; 4]> {
    let tint = policy.field_highlight.filter(|_| !policy.print_intent)?;
    if !annot.is_widget() {
        return None;
    }
    let rect = annot.rect?;
    let required = highlight_of(form.get(doc)?, annot.id?)?;
    let path = PathBuilder::from_rect(device_rect(rect, 0.0)?);
    canvas.fill(
        &path,
        &brush(tint.fill),
        FillRule::Winding,
        base_ctm,
        no_clip(),
    );
    required.then_some(tint.required_border)
}

/// Stroke a required field's border just inside its `/Rect`.
pub(super) fn paint_highlight_border(
    annot: &Annotation,
    base_ctm: Transform,
    colour: [u8; 4],
    canvas: &mut Canvas<'_>,
) {
    let Some(r) = annot.rect.and_then(|r| device_rect(r, 0.5)) else {
        return;
    };
    let path = PathBuilder::from_rect(r);
    let stroke = Stroke {
        width: 1.0,
        ..Stroke::default()
    };
    canvas.stroke(&path, &brush(colour), &stroke, base_ctm, no_clip());
}

/// `rect` inset by `inset` points, as a `tiny_skia` rectangle.
#[allow(clippy::cast_possible_truncation)] // page coordinates fit an f32.
fn device_rect(rect: Rect, inset: f64) -> Option<tiny_skia::Rect> {
    let (x0, x1) = (rect.llx.min(rect.urx), rect.llx.max(rect.urx));
    let (y0, y1) = (rect.lly.min(rect.ury), rect.lly.max(rect.ury));
    tiny_skia::Rect::from_ltrb(
        (x0 + inset) as f32,
        (y0 + inset) as f32,
        (x1 - inset) as f32,
        (y1 - inset) as f32,
    )
}

fn brush([r, g, b, a]: [u8; 4]) -> BrushSpec {
    // Half a level up, so `BrushSpec::solid`'s truncation lands back on the
    // byte asked for.
    let unit = |v: u8| (f32::from(v) + 0.5) / 255.0;
    BrushSpec::solid(
        Rgb {
            r: unit(r),
            g: unit(g),
            b: unit(b),
        },
        f32::from(a) / 255.0,
        BlendMode::SourceOver,
    )
}

const fn no_clip() -> ClipRef<'static> {
    ClipRef {
        mask: None,
        id: None,
    }
}
