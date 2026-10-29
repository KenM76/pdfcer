//! Rasterize one computed page of a dynamic XFA form (Pass 647.4;
//! decisions 203, 204, 207).
//!
//! A dynamic form's PDF pages are a placeholder; its real pages exist only
//! as [`pdfcer_core::xfa::layout_template`] output, and their count need not
//! match the file's. So this is a separate entry point keyed by a
//! [`LayoutPage`], not a branch inside [`crate::render_page`]: the caller
//! chooses which page set it is showing. Nothing is written to the file.

use pdfcer_core::content::ContentStream;
use pdfcer_core::object::Dict;
use pdfcer_core::xfa::{LayoutPage, PaintNote, PaintOptions, Template, paint_page};
use tiny_skia::{Pixmap, Transform};

use crate::{
    DocumentView, MAX_PIXMAP_EDGE, PageBackdrop, RenderError, RenderOptions, RenderedPage, canvas,
    compositor, gstate, interpret,
};

/// A rendered XFA page and what its drawing left out.
#[derive(Debug)]
#[non_exhaustive]
pub struct RenderedXfaPage {
    /// Pixels and render diagnostics, as for a PDF page.
    pub page: RenderedPage,
    /// What [`paint_page`] did not draw or drew approximately (rule 4: the
    /// shell discloses these off-canvas).
    pub notes: Vec<PaintNote>,
}

/// Draw `page` of `template`'s layout at `scale` device pixels per point.
///
/// `view` is the document the template came from; the synthesized content's
/// only resources are the standard-14 fonts [`paint_page`] names, so it
/// supplies nothing yet, but it is where embedded fonts and images will
/// resolve. The page is white-backed unless `options`
/// asks for [`PageBackdrop::Transparent`].
///
/// # Errors
///
/// [`RenderError::BadRasterSize`] when the page is empty or exceeds
/// [`MAX_PIXMAP_EDGE`] at `scale`; [`RenderError::RasterizerLimit`] if the
/// rasterizer panics; [`RenderError::Cancelled`] on cancellation.
pub fn render_xfa_page(
    view: &DocumentView<'_>,
    template: &Template,
    page: &LayoutPage,
    paint: &PaintOptions,
    scale: f32,
    options: &RenderOptions,
) -> Result<RenderedXfaPage, RenderError> {
    let mut painted = paint_page(template, page, paint);
    let content = std::mem::take(&mut painted.content);
    let resources = std::mem::take(&mut painted.resources);
    let rendered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        rasterize(view, content, &resources, page, scale, options)
    }))
    .unwrap_or_else(|payload| {
        Err(RenderError::RasterizerLimit {
            scale,
            panic_message: crate::panic_text(&payload),
        })
    })?;
    Ok(RenderedXfaPage {
        page: rendered,
        notes: painted.notes,
    })
}

fn rasterize(
    view: &DocumentView<'_>,
    content: Vec<u8>,
    resources: &Dict,
    page: &LayoutPage,
    scale: f32,
    options: &RenderOptions,
) -> Result<RenderedPage, RenderError> {
    let edge = |pt: f64| {
        let px = (pt * f64::from(scale)).ceil();
        // Negative, NaN and huge sizes all land outside 1..=MAX_PIXMAP_EDGE.
        if px.is_finite() && px >= 1.0 && px <= f64::from(MAX_PIXMAP_EDGE) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            // range-checked above
            Some(px as u32)
        } else {
            None
        }
    };
    let (Some(width), Some(height)) = (edge(page.width), edge(page.height)) else {
        return Err(RenderError::BadRasterSize {
            width: 0,
            height: 0,
        });
    };
    let Some(mut pixmap) = Pixmap::new(width, height) else {
        return Err(RenderError::BadRasterSize { width, height });
    };
    // Points are y-up from the page's bottom edge, as in PDF user space.
    #[allow(clippy::cast_possible_truncation)] // a page edge in points fits f32
    let ctm = Transform::from_row(scale, 0.0, 0.0, -scale, 0.0, page.height as f32 * scale);
    let parsed = ContentStream::parse(content)?;
    let diagnostics = {
        let mut canvas = canvas::Canvas::paint(&mut pixmap);
        interpret::run_on(
            view,
            &parsed,
            resources,
            &options.fonts,
            gstate::GraphicsState::default_with_ctm(ctm),
            &mut canvas,
            options.cancel.as_ref(),
            options.policy(),
            compositor::BlendSpace::Additive,
        )
    };
    if options
        .cancel
        .as_ref()
        .is_some_and(crate::cancel::RenderCancel::is_cancelled)
    {
        return Err(RenderError::Cancelled);
    }
    if options.effective_backdrop() == PageBackdrop::White {
        crate::flatten_page_group_over_white(&mut pixmap);
    }
    Ok(RenderedPage {
        pixmap,
        diagnostics,
    })
}

#[cfg(test)]
mod tests;
