//! `print --separations`: one grey plate per ink instead of the composite
//! page, through the renderer's separation preview with every other plate
//! left off (ISO 32000-2 §10.8.3).
//!
//! A plate's grey is the darkest channel of that ink alone printed on white,
//! so 100% ink is black and a tint is its share of grey: an approximation of
//! the plate's density, said so on stderr. Each plate is labelled with its
//! ink name at its foot (see `label`).

// Called only by the `#[cfg(windows)]` `cmd_print`; pure, so compiled and
// tested everywhere.
#![cfg_attr(not(windows), allow(dead_code))]

use pdfcer_core::page_tree::Page;
use pdfcer_render::tiny_skia::{Pixmap, PixmapPaint, Transform};
use pdfcer_render::{DocumentView, RenderOptions, SeparationPreview};

/// One ink's plate.
pub(crate) struct Plate {
    pub ink: String,
    pub pixmap: Pixmap,
}

/// Render `page` once per ink as a labelled grey plate. A page the
/// separation preview cannot composite in ink (the colorant buffer refused
/// for memory) comes back as its composite render with an empty ink name.
pub(crate) fn render_plates(
    view: &DocumentView<'_>,
    page: &Page,
    scale: f32,
    options: &RenderOptions,
) -> Result<Vec<Plate>, String> {
    let probe = options
        .clone()
        .with_separation_preview(Some(SeparationPreview::new()));
    let first = pdfcer_render::render_page_with_view(view, page, scale, &probe)
        .map_err(|e| e.to_string())?;
    let inks = first.diagnostics.inks.clone();
    if inks.is_empty() {
        return Ok(vec![Plate {
            ink: String::new(),
            pixmap: first.pixmap,
        }]);
    }
    let mut plates = Vec::with_capacity(inks.len());
    for ink in &inks {
        let others = inks.iter().filter(|i| *i != ink).cloned();
        let only = options
            .clone()
            .with_separation_preview(Some(SeparationPreview::new().with_hidden(others)));
        let mut pixmap = pdfcer_render::render_page_with_view(view, page, scale, &only)
            .map_err(|e| e.to_string())?
            .pixmap;
        to_plate_grey(pixmap.data_mut());
        label(&mut pixmap, ink, scale, page);
        plates.push(Plate {
            ink: ink.clone(),
            pixmap,
        });
    }
    Ok(plates)
}

/// Say which plates page `index` (0-based) printed as.
pub(crate) fn report_plates(index: usize, plates: &[Plate]) {
    if plates.iter().all(|p| p.ink.is_empty()) {
        eprintln!(
            "pdfcer: page {}: separations unavailable (the ink buffer was refused for memory); printed as the composite page.",
            index + 1
        );
        return;
    }
    let names: Vec<&str> = plates.iter().map(|p| p.ink.as_str()).collect();
    eprintln!(
        "pdfcer: page {}: {} plate(s): {}. Plate greys approximate ink density.",
        index + 1,
        plates.len(),
        names.join(", ")
    );
}

/// Premultiplied RGBA over white, in place: each pixel becomes the grey of
/// its darkest channel.
pub(crate) fn to_plate_grey(rgba: &mut [u8]) {
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3];
        // Composite over white first: c + (255 - a).
        let over = |c: u8| c.saturating_add(255 - a);
        let g = over(px[0]).min(over(px[1])).min(over(px[2]));
        px[0] = g;
        px[1] = g;
        px[2] = g;
        px[3] = 255;
    }
}

/// Draw `ink` in 6 pt Helvetica black at the plate's foot: right-aligned
/// just inside the trim's right edge when the page has a marks band below
/// its trim (clear of the page-info line and the corner marks), else at the
/// lower-left corner.
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
    pdfcer_render::render_page(&doc, &page, scale)
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
        assert!(p.data().chunks_exact(4).any(|px| px[0] < 128));
    }
}
