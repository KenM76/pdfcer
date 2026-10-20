//! G211: `is_hidden_under` must agree with the renderer for every object,
//! including one whose INNER `/OC` is shown inside a hidden outer section and
//! form leaves hidden by a section around the form's `Do` or by the form's own
//! `/OC`. Each square is black on white; "painted" is read off the raster.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::annot::optional_content_default_off;
use pdfcer_core::document::Document;
use pdfcer_core::object::ObjId;
use pdfcer_core::page_tree::pages;
use pdfcer_core::vector::{Bounds, ImageSource, Matrix, VectorObject, decompose_page};
use pdfcer_render::render_page;

const OUTER: ObjId = ObjId::new(4, 0);
const INNER: ObjId = ObjId::new(5, 0);

fn form(content: &str, extra: &str) -> String {
    format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 300 40]{extra} /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

/// Outer is OFF, Inner is ON. Squares at x = 10, 60, 110, 160, 210, 260:
/// Outer>Inner (hidden), Inner (shown), F1 under Outer (hidden), F2 with
/// its own `/OC Outer` (hidden), F3 under Inner holding an Outer section
/// (hidden), F4 under Inner (shown).
fn doc() -> Document {
    let page = "/OC /Outer BDC /OC /Inner BDC 0 g 10 10 20 20 re f EMC EMC \
                /OC /Inner BDC 0 g 60 10 20 20 re f EMC \
                /OC /Outer BDC /F1 Do EMC /F2 Do \
                /OC /Inner BDC /F3 Do /F4 Do EMC";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [4 0 R 5 0 R] \
         /D << /OFF [4 0 R] >> >> >>"
            .to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 40] /Contents 6 0 R \
         /Resources << /Properties << /Outer 4 0 R /Inner 5 0 R >> \
         /XObject << /F1 7 0 R /F2 8 0 R /F3 9 0 R /F4 10 0 R >> >> >>"
            .to_string(),
        "<< /Type /OCG /Name (Outer) >>".to_string(),
        "<< /Type /OCG /Name (Inner) >>".to_string(),
        format!("<< /Length {} >>\nstream\n{page}\nendstream", page.len()),
        form("0 g 110 10 20 20 re f", ""),
        form("0 g 160 10 20 20 re f", " /OC 4 0 R"),
        form(
            "/OC /O BDC 0 g 210 10 20 20 re f EMC",
            " /Resources << /Properties << /O 4 0 R >> >>",
        ),
        form("0 g 260 10 20 20 re f", ""),
    ];
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for o in offsets {
        buf.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    Document::from_bytes(buf).expect("parses")
}

/// Whether the raster is dark at `b`'s centre (page height 40, scale 1).
fn painted(pixmap: &tiny_skia::Pixmap, b: Bounds) -> bool {
    let x = ((b.min.x + b.max.x) / 2.0) as u32;
    let y = (40.0 - (b.min.y + b.max.y) / 2.0) as u32;
    pixmap.pixel(x, y).expect("in page").green() < 128
}

#[test]
fn every_object_is_hidden_exactly_when_the_renderer_skips_it() {
    let doc = doc();
    let view = doc.view();
    let page = &pages(&doc).unwrap()[0];
    let raster = render_page(&doc, page, 1.0).unwrap().pixmap;
    let off = optional_content_default_off(&view);
    assert!(off.contains(&OUTER) && !off.contains(&INNER));
    let model = decompose_page(&view, page, Matrix::IDENTITY).unwrap();

    let mut hidden = Vec::new();
    for o in &model.objects {
        if matches!(o, VectorObject::Image(i) if i.source == ImageSource::Form) {
            continue;
        }
        let h = o.is_hidden_under(&view, &off);
        assert_eq!(painted(&raster, o.page_bbox()), !h, "{o:?}");
        hidden.push(h);
    }
    for leaf in &model.leaves {
        let h = leaf.is_hidden_under(&view, &off);
        assert_eq!(painted(&raster, leaf.object.page_bbox()), !h, "{leaf:?}");
        hidden.push(h);
    }
    assert_eq!(hidden, [true, false, true, true, true, false]);
    // The case the request named: innermost is the shown Inner.
    assert_eq!(model.objects[0].oc(), Some(INNER));
    assert_eq!(model.objects[0].oc_chain(), [OUTER, INNER]);
    assert_eq!(model.leaves[2].oc_chain(), [INNER, OUTER]);
}
