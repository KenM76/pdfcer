//! `print`'s vector route: a page goes to the printer as an Enhanced
//! Metafile when `pdfcer_render::emf::print_metafile` says the metafile is
//! the whole page, and as a raster otherwise. Each page's route is reported
//! on stderr (rule 4).

// Called only by the `#[cfg(windows)]` `cmd_print`; pure, so compiled and
// tested everywhere.
#![cfg_attr(not(windows), allow(dead_code))]

use pdfcer_core::page_tree::Page;
use pdfcer_render::emf::{PrintMetafileError, print_metafile};
use pdfcer_render::{DocumentView, RenderOptions};

/// The page as a printable metafile at `scale` device pixels per point, or
/// `Err` with the reason it prints as an image.
pub(crate) fn vector_page(
    view: &DocumentView<'_>,
    page: &Page,
    scale: f32,
    options: &RenderOptions,
) -> Result<Vec<u8>, String> {
    print_metafile(view, page, options, scale * 72.0).map_err(|e| match e {
        PrintMetafileError::Raster(why) => why.as_str().to_owned(),
        other => other.to_string(),
    })
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
