//! Painting one annotation in the appearance the render asked for: `/N`,
//! `/R` or `/D` (ISO 32000-1 §12.5.5 Table 168), from its `/AP`, a standard
//! icon, its field data or its own geometry.

use pdfcer_core::annot::{Annotation, Appearance, AppearanceKind, select_appearance};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::Object;
use pdfcer_core::page_tree::Page;
use pdfcer_core::view::DocumentView;
use pdfcer_core::widget_draw::SynthesizedKind;
use tiny_skia::Transform;

use super::{markup, paint_appearance, paint_named_icon, widget};
use crate::canvas::Canvas;
use crate::font::{FontEnvironment, RenderPolicy};
use crate::interpret::Diagnostics;

/// The appearance kind the render asks of `annot`: its
/// [`crate::RenderOptions::appearance_states`] entry, else
/// [`crate::RenderOptions::widget_state`] for a widget, else `/N`.
fn requested(annot: &Annotation, policy: RenderPolicy<'_>) -> AppearanceKind {
    let explicit = annot.id.and_then(|id| {
        policy
            .appearance_states
            .iter()
            .find(|(of, _)| *of == id)
            .map(|&(_, kind)| kind)
    });
    match explicit {
        Some(kind) => kind,
        None if annot.is_widget() => policy.widget_state,
        None => AppearanceKind::Normal,
    }
}

/// `annot`'s appearance of `kind`, and whether it fell back to `/N`. An
/// annotation that is not an indirect object cannot be re-read, so it keeps
/// its `/N` and counts as a fallback.
fn select(
    doc: &DocumentView<'_>,
    annot: &Annotation,
    kind: AppearanceKind,
    policy: RenderPolicy<'_>,
    diag: &mut Diagnostics,
) -> (Appearance, bool) {
    if kind == AppearanceKind::Normal {
        return (annot.appearance.clone(), false);
    }
    let Some(dict) = annot
        .id
        .and_then(|id| doc.value(id))
        .and_then(Object::as_dict)
    else {
        return (annot.appearance.clone(), true);
    };
    let chosen = select_appearance(doc, dict, kind, policy.missing_as);
    if chosen.highlight_overrides_down
        && let Some(id) = annot.id
    {
        diag.note_annotation(&format!(
            "widget {id} drawn in its down appearance on request, though its /H is not P and \
             Table 188 says a viewer would not show /D when it is pressed",
        ));
    }
    (chosen.appearance, chosen.fell_back_to_normal)
}

/// Count and note that `annot` was asked for `kind` and drawn from `/N`.
fn report_fallback(annot: &Annotation, kind: AppearanceKind, diag: &mut Diagnostics) {
    diag.annotations_state_fallback += 1;
    let id = annot
        .id
        .map_or_else(|| "(direct)".to_owned(), |id| id.to_string());
    let key = String::from_utf8_lossy(kind.ap_key()).into_owned();
    diag.note_annotation(&format!(
        "annotation {id} has no /{key} appearance - drawn from /N instead (12.5.5 Table 168 \
         defaults /{key} to /N)",
    ));
}

/// Paint `annot` in its requested appearance, or count why it was not.
/// `in_scope` false counts it without painting.
#[allow(clippy::too_many_arguments)] // every argument is placement input.
pub(super) fn paint_annotation(
    doc: &DocumentView<'_>,
    page: &Page,
    ctm: Transform,
    fonts: &FontEnvironment,
    annot: &Annotation,
    in_scope: bool,
    form: &mut widget::FormCache,
    diag: &mut Diagnostics,
    canvas: &mut Canvas<'_>,
    cancel: Option<&crate::cancel::RenderCancel>,
    policy: RenderPolicy,
) {
    let kind = requested(annot, policy);
    let (appearance, fell_back) = select(doc, annot, kind, policy, diag);
    match appearance {
        Appearance::Normal { stream_id } => {
            // `annotations_painted` and the placement counters only mean
            // something when painting is enabled; when suppressed the
            // annotation is disclosed by `annotations_total` and
            // `annotations_out_of_scope` alone. A `/NeedAppearances` redraw
            // replaces `/N` only, never an `/R` or `/D` stream.
            let shows_normal = kind == AppearanceKind::Normal || fell_back;
            if in_scope
                && shows_normal
                && widget::paint_for_need_appearances(
                    doc, ctm, fonts, annot, form, diag, canvas, cancel, policy,
                )
            {
                diag.widgets_redrawn_for_need_appearances += 1;
            } else if in_scope {
                paint_appearance(
                    doc, page, ctm, fonts, annot, stream_id, diag, canvas, cancel, policy,
                );
            }
            if in_scope && fell_back {
                report_fallback(annot, kind, diag);
            }
        }
        // `Pass 289.0`: an annotation that NAMES A STANDARD ICON is drawn
        // from pdfcer's own artwork (§12.5.6.4 Table 172 and §12.5.6.12
        // Table 181 put that on the READER with a `shall`). A widget is
        // drawn from its own data, and a markup annotation from its own
        // geometry (decision 203); anything that would need guessing stays
        // `R43` named-not-painted. The counter increments either way: the
        // FILE had no `/AP`, which is what the demand signal measures.
        Appearance::None => {
            *diag
                .annotations_without_ap
                .entry(annot.subtype_label())
                .or_insert(0) += 1;
            if !in_scope {
                return;
            }
            let mut built_in_state = false;
            if paint_named_icon(doc, ctm, fonts, annot, diag, canvas, cancel, policy) {
                diag.annotations_icon_painted += 1;
            } else if let Some(drawn) = widget::paint_synthesized_widget(
                doc, ctm, fonts, annot, form, kind, diag, canvas, cancel, policy,
            ) {
                diag.annotations_widget_synthesized += 1;
                built_in_state = drawn == SynthesizedKind::PushButton;
            } else {
                markup::paint_from_geometry(doc, ctm, fonts, annot, diag, canvas, cancel, policy);
            }
            if kind != AppearanceKind::Normal && !built_in_state {
                report_fallback(annot, kind, diag);
            }
        }
        // §12.5.5 NOTE 3: an /AS that could not be resolved — display
        // nothing, counted separately (the annotation HAS appearances; only
        // selection failed).
        Appearance::StateUnresolved => {
            diag.annotations_appearance_state_missing += 1;
            if in_scope && fell_back {
                report_fallback(annot, kind, diag);
            }
        }
    }
}
