//! `print`'s vector route: a page goes to the printer as an Enhanced
//! Metafile from the renderer's EMF writer when the metafile carries every
//! part of the page as geometry, or as the page's own images, and as a
//! raster otherwise. Each page's route is reported on stderr (rule 4).
//!
//! The test is "nothing the EMF writer had to rasterise or approximate":
//! a page that needed a scratch raster for transparency, a flattened blend,
//! a raster gradient, an approximated overprint or blend, or an on-screen
//! substitute for a subtractive blending space would print differently from
//! its raster, so it prints as the raster instead.

// Called only by the `#[cfg(windows)]` `cmd_print`; pure, so compiled and
// tested everywhere.
#![cfg_attr(not(windows), allow(dead_code))]

use pdfcer_core::page_tree::Page;
use pdfcer_render::emf::{EmfOptions, EmfOutcome};
use pdfcer_render::{DocumentView, RenderOptions};

/// Why `outcome`'s page must print as an image, or `None` when its metafile
/// is the whole page.
pub(crate) fn raster_reason(outcome: &EmfOutcome) -> Option<&'static str> {
    let t = &outcome.tally;
    let reasons = [
        (outcome.ops_rasterised_for_alpha, "semi-transparent paint"),
        (outcome.layers_rasterised, "transparency group"),
        (outcome.blends_flattened, "blend mode"),
        (outcome.blend_modes_dropped, "blend mode"),
        (outcome.gradients_rasterised, "shading"),
        (t.overprint_approximated as usize, "overprint"),
        (t.nonseparable_approximated as usize, "blend mode"),
        (
            t.non_isolated_groups_isolated as usize,
            "transparency group",
        ),
        (
            t.colorant_buffer_on_screen as usize,
            "blending colour space",
        ),
        (t.tiling_patterns as usize, "tiling pattern"),
        (t.shadings_rasterised as usize, "shading"),
    ];
    if let Some((_, why)) = reasons.iter().find(|(n, _)| *n > 0) {
        return Some(why);
    }
    // Every embedded raster must be one of the page's own images.
    (outcome.rasters_embedded > outcome.images_embedded).then_some("transparency")
}

/// The page as a printable metafile at `scale` device pixels per point, or
/// `Err` with the reason it prints as an image. Images inside the metafile
/// are sampled at the same resolution the raster route would use.
pub(crate) fn vector_page(
    view: &DocumentView<'_>,
    page: &Page,
    scale: f32,
    options: &RenderOptions,
) -> Result<Vec<u8>, String> {
    let emf = EmfOptions::default().with_raster_dpi(scale * 72.0);
    let export = pdfcer_render::emf::export_emf_view(view, page, options, &emf)
        .map_err(|e| format!("vector export failed: {e}"))?;
    match raster_reason(&export.outcome) {
        None => Ok(export.emf),
        Some(why) => Err(why.to_owned()),
    }
}

/// Say how page `index` (0-based) printed: `vector`, or `raster (why)`.
pub(crate) fn report_route(index: usize, route: &Result<Vec<u8>, String>) {
    match route {
        Ok(_) => eprintln!("pdfcer: page {}: vector", index + 1),
        Err(why) => eprintln!("pdfcer: page {}: raster ({why})", index + 1),
    }
}

/// Why a whole job prints as images: the clause after "every sheet prints
/// as an image".
pub(crate) fn why_all_raster(
    output: &crate::print_output::PrintOutput,
    imposed: bool,
) -> &'static str {
    use crate::print_output::ColorArg;
    if output.print_as_image {
        " (--print-as-image)"
    } else if output.separations {
        " (separation plates)"
    } else if imposed {
        " (imposed sheets)"
    } else if output.color == ColorArg::Grayscale {
        " (--color grayscale)"
    } else if output.color == ColorArg::BlackOnly {
        " (--color black-only)"
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdfcer_core::document::Document;

    fn one_page(content: &str, resources: &str) -> Document {
        let bodies = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R \
                 /Resources {resources} >>"
            ),
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            ),
        ];
        let mut buf = b"%PDF-1.7\n".to_vec();
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
        Document::from_bytes(buf).expect("fixture loads")
    }

    fn route(doc: &Document) -> Result<Vec<u8>, String> {
        let page = pdfcer_core::page_tree::pages(doc).expect("pages").remove(0);
        let view = DocumentView::new(doc, doc.bytes(), doc.version());
        vector_page(&view, &page, 2.0, &RenderOptions::default())
    }

    #[test]
    fn an_opaque_drawing_prints_as_vector() {
        let doc = one_page(
            "1 0 0 rg 10 10 50 50 re f 0 G 2 w 0 0 m 100 100 l S",
            "<< >>",
        );
        let emf = route(&doc).expect("vector");
        // EMR_HEADER (type 1) opens every metafile.
        assert_eq!(&emf[..4], &1u32.to_le_bytes());
    }

    #[test]
    fn a_semi_transparent_fill_falls_back_to_raster() {
        let doc = one_page(
            "/G0 gs 1 0 0 rg 10 10 50 50 re f",
            "<< /ExtGState << /G0 << /ca 0.5 >> >> >>",
        );
        assert_eq!(route(&doc), Err("semi-transparent paint".to_owned()));
    }

    #[test]
    fn a_multiply_blend_falls_back_to_raster() {
        let doc = one_page(
            "0 0 1 rg 0 0 60 60 re f /G0 gs 1 0 0 rg 30 30 60 60 re f",
            "<< /ExtGState << /G0 << /BM /Multiply >> >> >>",
        );
        assert_eq!(route(&doc), Err("blend mode".to_owned()));
    }
}
