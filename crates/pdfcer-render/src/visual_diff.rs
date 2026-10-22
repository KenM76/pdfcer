//! Before/after visual comparison of two versions of a document: each page
//! rendered at the same scale and options, scored by SSIM and by how many
//! pixels moved past a threshold. Built for the optimize report (what did
//! the size reduction cost on screen?) and usable for any two revisions
//! with the same page count.

use pdfcer_core::document::Document;
use pdfcer_core::optimize::structural_similarity;
use pdfcer_core::page_tree::{self, PageTreeError};
use tiny_skia::Pixmap;

use crate::{RenderError, RenderOptions, RenderedPage, render_page_with};

/// How [`visual_diff`] renders and what counts as a changed pixel.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct VisualDiffOptions {
    /// Device pixels per user-space unit (`dpi / 72`). Default `1.0`.
    pub scale: f32,
    /// A pixel is changed when any of R, G, B moved by more than this
    /// (0-255). Default 8, above re-encode and antialiasing noise.
    pub threshold: u8,
    /// Options for both renders. Default: [`RenderOptions::default`].
    pub render: RenderOptions,
}

impl Default for VisualDiffOptions {
    fn default() -> Self {
        Self {
            scale: 1.0,
            threshold: 8,
            render: RenderOptions::default(),
        }
    }
}

impl VisualDiffOptions {
    /// Render at `scale` device pixels per point.
    #[must_use]
    pub fn with_scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    /// Count a pixel changed past `threshold` levels.
    #[must_use]
    pub fn with_threshold(mut self, threshold: u8) -> Self {
        self.threshold = threshold;
        self
    }
}

/// The smallest pixel rectangle holding every changed pixel, origin top-left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChangedRegion {
    /// Left edge, pixels.
    pub x: u32,
    /// Top edge, pixels.
    pub y: u32,
    /// Width, pixels (at least 1).
    pub width: u32,
    /// Height, pixels (at least 1).
    pub height: u32,
}

/// One page's comparison.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PageDiff {
    /// SSIM of the after render against the before, on BT.601 luma; 1.0 is
    /// identical. See [`structural_similarity`].
    pub ssim: f64,
    /// The largest change of any channel at any pixel, 0-255.
    pub max_delta: u8,
    /// Pixels past [`VisualDiffOptions::threshold`].
    pub changed_pixels: u64,
    /// Pixels compared.
    pub pixels: u64,
    /// Where the changed pixels are; `None` when there are none.
    pub changed_region: Option<ChangedRegion>,
}

impl PageDiff {
    /// Whether any pixel moved past the threshold.
    #[must_use]
    pub fn changed(&self) -> bool {
        self.changed_pixels > 0
    }
}

/// Why a page could not be compared.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum VisualDiffError {
    /// The page exists in only one of the two documents.
    #[error("the page is missing from the {side} document")]
    MissingPage {
        /// `"before"` or `"after"`.
        side: &'static str,
    },
    /// One of the renders failed.
    #[error("the {side} render failed: {source}")]
    Render {
        /// `"before"` or `"after"`.
        side: &'static str,
        /// The render failure.
        #[source]
        source: RenderError,
    },
    /// The two renders have different pixel sizes: the page's box changed.
    #[error("the page size changed from {before:?} to {after:?} pixels")]
    SizeChanged {
        /// Before, (width, height).
        before: (u32, u32),
        /// After, (width, height).
        after: (u32, u32),
    },
}

/// Compare every page of `before` with the same page of `after`.
///
/// One entry per page of the longer document, in page order; a page only
/// one side has is [`VisualDiffError::MissingPage`].
///
/// # Errors
///
/// [`PageTreeError`] when either page tree cannot be read.
pub fn visual_diff(
    before: &Document,
    after: &Document,
    options: &VisualDiffOptions,
) -> Result<Vec<Result<PageDiff, VisualDiffError>>, PageTreeError> {
    let (a, b) = (page_tree::pages(before)?, page_tree::pages(after)?);
    let render = |doc: &Document, page, side| {
        render_page_with(doc, page, options.scale, &options.render)
            .map_err(|source| VisualDiffError::Render { side, source })
    };
    Ok((0..a.len().max(b.len()))
        .map(|i| {
            let pa = a
                .get(i)
                .ok_or(VisualDiffError::MissingPage { side: "before" })?;
            let pb = b
                .get(i)
                .ok_or(VisualDiffError::MissingPage { side: "after" })?;
            diff_rendered(
                &render(before, pa, "before")?,
                &render(after, pb, "after")?,
                options.threshold,
            )
        })
        .collect())
}

/// Compare two renders of the same page.
///
/// # Errors
///
/// [`VisualDiffError::SizeChanged`] when the pixel sizes differ.
pub fn diff_rendered(
    before: &RenderedPage,
    after: &RenderedPage,
    threshold: u8,
) -> Result<PageDiff, VisualDiffError> {
    let (pa, pb) = same_size(&before.pixmap, &after.pixmap)?;
    let (w, h) = (pa.width(), pa.height());
    let (mut max_delta, mut changed) = (0_u8, 0_u64);
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for (i, d) in deltas(pa, pb).enumerate() {
        max_delta = max_delta.max(d);
        if d > threshold {
            changed += 1;
            let (x, y) = (i as u32 % w, i as u32 / w);
            bounds = Some(bounds.map_or((x, y, x, y), |(x0, y0, x1, y1)| {
                (x0.min(x), y0.min(y), x1.max(x), y1.max(y))
            }));
        }
    }
    let ssim = structural_similarity(&rgb(pa), &rgb(pb), w, h, 3).unwrap_or(1.0);
    Ok(PageDiff {
        ssim,
        max_delta,
        changed_pixels: changed,
        pixels: u64::from(w) * u64::from(h),
        changed_region: bounds.map(|(x0, y0, x1, y1)| ChangedRegion {
            x: x0,
            y: y0,
            width: x1 - x0 + 1,
            height: y1 - y0 + 1,
        }),
    })
}

/// A picture of where two renders differ: the after render washed out to
/// light grey, with every pixel past `threshold` painted red, more opaque
/// the larger the change.
///
/// # Errors
///
/// [`VisualDiffError::SizeChanged`] when the pixel sizes differ.
pub fn diff_heatmap(
    before: &RenderedPage,
    after: &RenderedPage,
    threshold: u8,
) -> Result<Pixmap, VisualDiffError> {
    let (pa, pb) = same_size(&before.pixmap, &after.pixmap)?;
    let mut out = pb.clone();
    let ds: Vec<u8> = deltas(pa, pb).collect();
    for (px, d) in out.data_mut().as_chunks_mut::<4>().0.iter_mut().zip(ds) {
        let [r, g, b, _] = *px;
        let luma = (u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000;
        let grey = 191 + luma / 4;
        let t = if d > threshold {
            u32::from(d.max(64))
        } else {
            0
        };
        let mix = |to: u32| u8::try_from((grey * (255 - t) + to * t) / 255).unwrap_or(u8::MAX);
        *px = [mix(255), mix(0), mix(0), 255];
    }
    Ok(out)
}

fn same_size<'a>(
    a: &'a Pixmap,
    b: &'a Pixmap,
) -> Result<(&'a Pixmap, &'a Pixmap), VisualDiffError> {
    if (a.width(), a.height()) == (b.width(), b.height()) {
        Ok((a, b))
    } else {
        Err(VisualDiffError::SizeChanged {
            before: (a.width(), a.height()),
            after: (b.width(), b.height()),
        })
    }
}

/// Per pixel, the largest R/G/B change.
fn deltas<'a>(a: &'a Pixmap, b: &'a Pixmap) -> impl Iterator<Item = u8> + 'a {
    pixels(a)
        .zip(pixels(b))
        .map(|([r, g, b, _], [r2, g2, b2, _])| {
            r.abs_diff(r2).max(g.abs_diff(g2)).max(b.abs_diff(b2))
        })
}

fn pixels(p: &Pixmap) -> impl Iterator<Item = [u8; 4]> + '_ {
    p.data().as_chunks::<4>().0.iter().copied()
}

fn rgb(p: &Pixmap) -> Vec<u8> {
    pixels(p).flat_map(|[r, g, b, _]| [r, g, b]).collect()
}
