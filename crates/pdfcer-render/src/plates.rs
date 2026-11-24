//! A page as its separation plates: one grey image per ink, through the
//! separation preview with every other plate left off (ISO 32000-2
//! §10.8.3), each labelled with its ink name.
//!
//! A plate's grey is the darkest channel of that ink alone printed on
//! white, so 100% ink is black and a tint is its share of grey. That is an
//! approximation of the plate's density; a shell printing plates says so.

use pdfcer_core::page_tree::Page;
use tiny_skia::{Pixmap, PixmapPaint, Transform};

use crate::{DocumentView, RenderError, RenderOptions, SeparationPreview};

/// One ink's plate: its grey image, labelled with `ink` at its foot.
#[derive(Debug, Clone, PartialEq)]
pub struct Plate {
    /// The ink, as the file spells it (`Cyan` … `Black`, then each spot).
    pub ink: String,
    /// The plate at the requested scale: opaque grey, ink black.
    pub pixmap: Pixmap,
}

/// What [`render_plates`] produced for a page.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Plates {
    /// One plate per ink: the four process inks, then each spot in plane
    /// order.
    Separated(Vec<Plate>),
    /// The page could not be composited in ink (the colorant buffer was
    /// refused for memory, `Diagnostics::cmyk_buffer_refused`), so this is
    /// its ordinary composite render. Unlabelled and in colour.
    Composite(Pixmap),
}

/// Render `page` as a labelled grey plate per ink, four plates per render:
/// the page is composited in ink once per four inks, and each ink's plate is
/// read off that composite with the others left off. A process-only page
/// costs one render.
///
/// Each plate equals [`plate_pixmap`]'s for its ink (one render per ink)
/// when edges are not supersampled; supersampled, a plate pixel is the mean
/// of its fine pixels' greys rather than the grey of their mean colour.
///
/// # Errors
///
/// Any [`RenderError`] from a render.
pub fn render_plates(
    view: &DocumentView<'_>,
    page: &Page,
    scale: f32,
    options: &RenderOptions,
) -> Result<Plates, RenderError> {
    let first: Vec<String> = crate::PROCESS_INKS
        .iter()
        .map(|n| (*n).to_owned())
        .collect();
    let (inks, pixmap) = render_packed(view, page, scale, options, first.clone())?;
    if inks.is_empty() {
        return Ok(Plates::Composite(pixmap));
    }
    let mut plates = Vec::with_capacity(inks.len());
    unpack(&mut plates, &pixmap, &first, (scale, page));
    for group in inks.get(first.len()..).unwrap_or_default().chunks(4) {
        let (_, pixmap) = render_packed(view, page, scale, options, group.to_vec())?;
        unpack(&mut plates, &pixmap, group, (scale, page));
    }
    Ok(Plates::Separated(plates))
}

/// One render collapsing to `names`' plates ([`packed`]), and the page's
/// inks. No inks: the colorant buffer was refused and the pixmap is the
/// page's composite.
fn render_packed(
    view: &DocumentView<'_>,
    page: &Page,
    scale: f32,
    options: &RenderOptions,
    names: Vec<String>,
) -> Result<(Vec<String>, Pixmap), RenderError> {
    let mut options = options
        .clone()
        .with_separation_preview(Some(SeparationPreview::new()));
    options.ink_probe = None;
    options.collapse = crate::font::Collapse::Plates(names);
    let rendered = crate::render_page_with_view(view, page, scale, &options)?;
    Ok((rendered.diagnostics.inks, rendered.pixmap))
}

/// Split a [`packed`] raster into labelled plates for `names`, in order.
fn unpack(plates: &mut Vec<Plate>, packed: &Pixmap, names: &[String], (scale, page): (f32, &Page)) {
    for (channel, ink) in names.iter().enumerate() {
        let Some(mut pixmap) = Pixmap::new(packed.width(), packed.height()) else {
            continue;
        };
        for (dst, src) in pixmap
            .data_mut()
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(packed.data().as_chunks::<4>().0)
        {
            let grey = src.get(channel).copied().unwrap_or(255);
            *dst = [grey, grey, grey, 255];
        }
        label(&mut pixmap, ink, scale, page);
        plates.push(Plate {
            ink: ink.clone(),
            pixmap,
        });
    }
}

/// The colorant buffer as up to four plates, `names[c]`'s grey
/// ([`to_plate_grey`] of the page with every other ink left off) in RGBA
/// channel `c`; a channel with no name, or an ink this buffer does not
/// hold, is white. The bytes are not a colour: [`unpack`] reads them.
/// `None` when the raster cannot be allocated.
pub(crate) fn packed(
    buffer: &crate::cmyk_buffer::CmykBuffer,
    names: &[String],
    backdrop: crate::PageBackdrop,
) -> Option<Pixmap> {
    let mut out = Pixmap::new(buffer.width(), buffer.height())?;
    out.data_mut().fill(255);
    let roster = buffer.ink_names();
    for (channel, ink) in names.iter().enumerate().take(4) {
        if !roster.contains(ink) {
            continue;
        }
        let mut alone = buffer.clone();
        let others: Vec<String> = roster.iter().filter(|i| *i != ink).cloned().collect();
        alone.hide_inks(&others);
        let collapsed = match backdrop {
            crate::PageBackdrop::White => alone.to_srgb_over_white(),
            crate::PageBackdrop::Transparent => alone.to_srgb_transparent(),
        };
        let Some(mut plate) = collapsed else { continue };
        to_plate_grey(plate.data_mut());
        for (dst, src) in out
            .data_mut()
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(plate.data().as_chunks::<4>().0)
        {
            if let (Some(d), Some(g)) = (dst.get_mut(channel), src.first()) {
                *d = *g;
            }
        }
    }
    Some(out)
}

/// The page's inks in plate order, from a render under the separation
/// preview, and that render. No inks: the colorant buffer was refused.
#[doc(hidden)]
pub fn probe_inks(
    view: &DocumentView<'_>,
    page: &Page,
    scale: f32,
    options: &RenderOptions,
) -> Result<(Vec<String>, Pixmap), RenderError> {
    let probe = options
        .clone()
        .with_separation_preview(Some(SeparationPreview::new()));
    let first = crate::render_page_with_view(view, page, scale, &probe)?;
    Ok((first.diagnostics.inks, first.pixmap))
}

/// `ink`'s plate as opaque grey, unlabelled: the page with every other of
/// `inks` hidden, through [`to_plate_grey`].
#[doc(hidden)]
pub fn plate_pixmap(
    view: &DocumentView<'_>,
    page: &Page,
    scale: f32,
    options: &RenderOptions,
    inks: &[String],
    ink: &str,
) -> Result<Pixmap, RenderError> {
    let others = inks.iter().filter(|i| *i != ink).cloned();
    let only = options
        .clone()
        .with_separation_preview(Some(SeparationPreview::new().with_hidden(others)));
    let mut pixmap = crate::render_page_with_view(view, page, scale, &only)?.pixmap;
    to_plate_grey(pixmap.data_mut());
    Ok(pixmap)
}

/// Premultiplied RGBA over white, in place: each pixel becomes the grey of
/// its darkest channel, fully opaque. A trailing partial pixel is left
/// alone.
pub fn to_plate_grey(rgba: &mut [u8]) {
    for px in rgba.as_chunks_mut::<4>().0 {
        let [r, g, b, a] = px;
        let over = |c: u8| c.saturating_add(255 - *a);
        let grey = over(*r).min(over(*g)).min(over(*b));
        px.copy_from_slice(&[grey, grey, grey, 255]);
    }
}

/// Draw `ink` in 6 pt Helvetica black at the plate's foot: right-aligned
/// just inside the trim's right edge when the page has a marks band below
/// its trim (at least 10 pt between trim and crop, clear of the page-info
/// line and the corner marks), else at the lower-left corner.
fn label(plate: &mut Pixmap, ink: &str, scale: f32, page: &Page) {
    let Some(stamp) = label_pixmap(ink, scale) else {
        return;
    };
    let inset = (2.0 * scale).round() as i32;
    let y = plate.height() as i32 - stamp.height() as i32 - inset;
    let (crop, trim) = (page.crop_box, page.trim_box);
    let x = if trim.lly - crop.lly >= 10.0 {
        ((trim.urx - 3.0 - crop.llx) as f32 * scale).round() as i32 - stamp.width() as i32
    } else {
        inset
    };
    plate.draw_pixmap(
        x.max(0),
        y.max(0),
        stamp.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );
}

fn label_pixmap(ink: &str, scale: f32) -> Option<Pixmap> {
    let text: String = ink
        .chars()
        .map(|c| match c {
            '(' | ')' | '\\' => '_',
            ' '..='~' => c,
            _ => '?',
        })
        .collect();
    let width = 4.0 + 6.0 * 0.62 * text.chars().count() as f64;
    let content = format!("BT /F 6 Tf 1 1.6 Td ({text}) Tj ET");
    let pdf = one_page_pdf(width, 8.0, &content);
    let doc = pdfcer_core::document::Document::from_bytes(pdf).ok()?;
    let page = pdfcer_core::page_tree::pages(&doc)
        .ok()?
        .into_iter()
        .next()?;
    crate::render_page(&doc, &page, scale)
        .ok()
        .map(|r| r.pixmap)
}

fn one_page_pdf(w: f64, h: f64, content: &str) -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w:.2} {h:.2}] /Contents 4 0 R \
             /Resources << /Font << /F << /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
             >> >> >> >>"
        ),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
    ];
    let mut buf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = buf.len();
    buf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    buf
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::indexing_slicing)]
    use super::*;

    #[test]
    fn grey_is_the_darkest_channel_over_white() {
        let mut px = [
            0, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 0, 0, 64, 64, 128,
        ];
        to_plate_grey(&mut px);
        assert_eq!(&px[0..4], &[0, 0, 0, 255], "full cyan prints black");
        assert_eq!(&px[4..8], &[255, 255, 255, 255], "white stays white");
        assert_eq!(&px[8..12], &[255, 255, 255, 255], "transparent is white");
        assert_eq!(&px[12..16], &[127, 127, 127, 255], "half alpha over white");
    }

    #[test]
    fn a_label_renders() {
        let p = label_pixmap("Magenta", 2.0).expect("label renders");
        assert!(p.width() > 40);
        assert!(p.data().as_chunks::<4>().0.iter().any(|px| px[0] < 128));
    }
}
