//! White-margin detection: the user-space box around everything a page
//! paints, measured on a render, for "remove white margins" crops.
//!
//! The box is **inferred** (project rule 4): a shell applying it must say
//! so, off-canvas, and should state the limits listed on
//! [`white_margin_crop`].

use pdfcer_core::page_tree::{Page, Rect};

use crate::{DocumentView, RenderError, RenderOptions, render_page_with_view};

/// Resolution the margins are measured on, in device pixels per point
/// (144 dpi).
pub const WHITE_MARGIN_SCALE: f32 = 2.0;

/// A pixel whose red, green and blue are all at least this counts as paper.
/// Below 255 so a scanned near-white margin is not mistaken for content.
pub const WHITE_MARGIN_THRESHOLD: u8 = 250;

/// What [`white_margin_crop`] found on one page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WhiteMarginCrop {
    /// The box around every non-paper pixel, one pixel wider on each side,
    /// in default user space (already mapped back through `/Rotate`).
    pub content: Rect,
    /// `content` grown by the requested margin and clipped to the page's
    /// current crop box: the rectangle to hand to a page-box edit.
    pub crop: Rect,
}

/// Why [`white_margin_crop`] could not answer.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WhiteMarginError {
    /// The margin was negative, NaN or infinite.
    #[error("margin {0} must be zero or more points")]
    BadMargin(f64),
    /// The page failed to render.
    #[error("render failed: {0}")]
    Render(#[from] RenderError),
}

/// Fit a crop to what `page` paints, grown by `margin_pt` points.
///
/// Renders the page's visible region (its crop box) at
/// [`WHITE_MARGIN_SCALE`] with default [`RenderOptions`], so annotation
/// appearances count as content. Returns `Ok(None)` for a page that paints
/// nothing but paper; a shell should leave that page's box alone and say how
/// many pages were blank.
///
/// Limits a shell should disclose:
/// - the box is accurate to one render pixel (half a point);
/// - a near-white background darker than [`WHITE_MARGIN_THRESHOLD`] (a
///   grey scan, a tinted page) counts as content, so such a page may not
///   shrink at all;
/// - only the current crop box is searched, so the result never grows the
///   visible region.
///
/// # Errors
///
/// [`WhiteMarginError::BadMargin`] for a negative or non-finite margin;
/// [`WhiteMarginError::Render`] when the page does not render.
///
/// # Example
///
/// ```no_run
/// # fn run(view: &pdfcer_render::DocumentView<'_>, page: &pdfcer_core::page_tree::Page)
/// # -> Result<(), pdfcer_render::WhiteMarginError> {
/// match pdfcer_render::white_margin_crop(view, page, 6.0)? {
///     Some(found) => println!("crop to {:?}", found.crop),
///     None => println!("blank page; left uncropped"),
/// }
/// # Ok(()) }
/// ```
pub fn white_margin_crop(
    view: &DocumentView<'_>,
    page: &Page,
    margin_pt: f64,
) -> Result<Option<WhiteMarginCrop>, WhiteMarginError> {
    if !margin_pt.is_finite() || margin_pt < 0.0 {
        return Err(WhiteMarginError::BadMargin(margin_pt));
    }
    let rendered =
        render_page_with_view(view, page, WHITE_MARGIN_SCALE, &RenderOptions::default())?;
    let pix = &rendered.pixmap;
    let (w, h) = (pix.width() as usize, pix.height() as usize);
    let Some(extent) = ink_extent(pix.data(), w, h) else {
        return Ok(None);
    };
    let content = extent_to_user(page, extent, w, h);
    let grown = Rect::from_corners(
        content.llx - margin_pt,
        content.lly - margin_pt,
        content.urx + margin_pt,
        content.ury + margin_pt,
    );
    let crop = grown.intersection(&page.crop_box).unwrap_or(content);
    Ok(Some(WhiteMarginCrop { content, crop }))
}

/// Map a pixel extent, padded by one pixel each side, back to user space.
fn extent_to_user(
    page: &Page,
    (x0, y0, x1, y1): (usize, usize, usize, usize),
    w: usize,
    h: usize,
) -> Rect {
    let (x0, y0) = (x0.saturating_sub(1) as f64, y0.saturating_sub(1) as f64);
    let (x1, y1) = ((x1 + 1).min(w) as f64, (y1 + 1).min(h) as f64);
    let c = page.crop_box;
    let turned = matches!(page.rotate % 360, 90 | 270);
    let s = w as f64 / if turned { c.height() } else { c.width() };
    let a = device_to_user(c, page.rotate, s, x0, y0);
    let b = device_to_user(c, page.rotate, s, x1, y1);
    Rect::from_corners(a.0, a.1, b.0, b.1)
}

/// Bounds `[x0, x1) × [y0, y1)` of the pixels darker than paper in an RGBA
/// buffer, or `None` when every pixel is paper.
fn ink_extent(data: &[u8], w: usize, h: usize) -> Option<(usize, usize, usize, usize)> {
    let mut extent: Option<(usize, usize, usize, usize)> = None;
    for (i, px) in data.chunks_exact(4).enumerate().take(w * h) {
        let [r, g, b, _] = *px else { continue };
        if [r, g, b].iter().all(|&v| v >= WHITE_MARGIN_THRESHOLD) {
            continue;
        }
        let (x, y) = (i % w, i / w);
        extent = Some(match extent {
            None => (x, y, x + 1, y + 1),
            Some((a, b, c, d)) => (a.min(x), b.min(y), c.max(x + 1), d.max(y + 1)),
        });
    }
    extent
}

/// Map a device pixel (top-left origin, `s` px per point) on a page framed
/// by crop box `c` and turned clockwise by `/Rotate` back to user space.
fn device_to_user(c: Rect, rotate: u16, s: f64, x: f64, y: f64) -> (f64, f64) {
    let (u, v) = (x / s, y / s);
    match rotate % 360 {
        90 => (c.llx + v, c.lly + u),
        180 => (c.urx - u, c.lly + v),
        270 => (c.urx - v, c.ury - u),
        _ => (c.llx + u, c.ury - v),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_all_paper_buffer_has_no_ink() {
        let data = [255u8, 250, 251, 255].repeat(6);
        assert_eq!(ink_extent(&data, 3, 2), None);
    }

    #[test]
    fn ink_extent_is_half_open_around_the_dark_pixels() {
        let dark = [(1, 0), (2, 2)];
        let data: Vec<u8> = (0..4 * 3)
            .flat_map(|i| match dark.contains(&(i % 4, i / 4)) {
                true => [249, 255, 255, 255],
                false => [255; 4],
            })
            .collect();
        assert_eq!(ink_extent(&data, 4, 3), Some((1, 0, 3, 3)));
    }

    #[test]
    fn device_to_user_inverts_each_rotation() {
        let c = Rect::from_corners(10.0, 20.0, 110.0, 220.0);
        assert_eq!(device_to_user(c, 0, 1.0, 0.0, 0.0), (10.0, 220.0));
        assert_eq!(device_to_user(c, 90, 1.0, 0.0, 0.0), (10.0, 20.0));
        assert_eq!(device_to_user(c, 180, 1.0, 0.0, 0.0), (110.0, 20.0));
        assert_eq!(device_to_user(c, 270, 1.0, 0.0, 0.0), (110.0, 220.0));
        // One point right, two down, at 2 px per point.
        assert_eq!(device_to_user(c, 0, 2.0, 2.0, 4.0), (11.0, 218.0));
        assert_eq!(device_to_user(c, 90, 2.0, 2.0, 4.0), (12.0, 21.0));
    }
}
