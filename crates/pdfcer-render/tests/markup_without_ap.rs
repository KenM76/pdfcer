//! Decision 203: a markup annotation with no `/AP` is drawn from its own
//! geometry and colours when the page renders, and left blank (with a note)
//! when that would need guessing. Nothing is written to the file.

use crate::named_icon_without_ap::{ink, no_ap_pdf, render};
use pdfcer_core::document::Document;
use pdfcer_render::{MarkupWithoutAp, RenderOptions, RenderedPage};

fn one(annot: &str) -> RenderedPage {
    render(no_ap_pdf("4 0 R", annot, 1))
}

fn drawn(out: &RenderedPage, subtype: &str) -> usize {
    out.diagnostics
        .annotations_geometry_drawn
        .get(subtype)
        .copied()
        .unwrap_or(0)
}

/// The non-paper pixels' rows (pixmap y, top down) as `(min, max)`.
fn ink_rows(out: &RenderedPage) -> Option<(u32, u32)> {
    let w = out.pixmap.width();
    let rows: Vec<u32> = out
        .pixmap
        .pixels()
        .iter()
        .enumerate()
        .filter(|(_, p)| p.alpha() > 0 && (p.red(), p.green(), p.blue()) != (255, 255, 255))
        .map(|(i, _)| u32::try_from(i).unwrap() / w)
        .collect();
    Some((*rows.iter().min()?, *rows.iter().max()?))
}

fn pixel(out: &RenderedPage, x: u32, y_pdf: u32) -> (u8, u8, u8) {
    let y = out.pixmap.height() - 1 - y_pdf;
    let p = out.pixmap.pixel(x, y).unwrap().demultiply();
    (p.red(), p.green(), p.blue())
}

#[test]
fn an_ink_annotation_is_drawn_and_still_counted_as_having_no_ap() {
    let out = one(
        "<< /Type /Annot /Subtype /Ink /C [1 0 0] /InkList [[50 50 250 250]] \
         /Rect [40 40 260 260] >>",
    );
    assert!(ink(&out) > 0, "the stroke is on the page");
    assert_eq!(drawn(&out, "Ink"), 1);
    assert_eq!(out.diagnostics.annotations_without_ap.get("Ink"), Some(&1));
    assert_eq!(out.diagnostics.annotations_painted, 0, "not an /AP paint");
}

/// The spec's counterclockwise corners and Acrobat's reading order draw the
/// same highlight — neither a bow-tie.
#[test]
fn a_highlight_draws_the_same_in_both_quad_orders() {
    let quad = |q: &str| {
        one(&format!(
            "<< /Type /Annot /Subtype /Highlight /C [1 1 0] /QuadPoints [{q}] \
             /Rect [40 100 260 140] >>"
        ))
    };
    let spec = quad("50 110 250 110 250 130 50 130");
    let acrobat = quad("50 130 250 130 50 110 250 110");
    assert!(ink(&spec) > 3000, "a filled band, not a bow-tie outline");
    assert_eq!(spec.pixmap.data(), acrobat.pixmap.data());
    assert_eq!(drawn(&spec, "Highlight"), 1);
}

/// An underline sits in the lower part of its quad, not across it.
#[test]
fn an_underline_is_drawn_near_the_bottom_of_its_quad() {
    let out = one("<< /Type /Annot /Subtype /Underline /C [0 0 1] \
         /QuadPoints [50 130 250 130 50 110 250 110] /Rect [40 100 260 140] >>");
    let (top, _) = ink_rows(&out).expect("the underline is drawn");
    // Quad spans pdf y 110..130 = pixmap rows 170..190 of a 300-high page.
    assert!(
        top > 180,
        "ink starts at row {top}, in the quad's lower half"
    );
    assert_eq!(drawn(&out, "Underline"), 1);
}

/// §12.5.6.8: the square sits inside `/Rect` less `/RD`.
#[test]
fn a_square_is_inset_by_its_rect_differences() {
    let out = one(
        "<< /Type /Annot /Subtype /Square /C [0 0 1] /BS << /W 2 >> \
         /Rect [40 40 260 260] /RD [50 50 50 50] >>",
    );
    assert!(ink(&out) > 0);
    assert_eq!(
        pixel(&out, 41, 150),
        (255, 255, 255),
        "nothing on /Rect's edge"
    );
    assert_ne!(
        pixel(&out, 91, 150),
        (255, 255, 255),
        "the inset edge is drawn"
    );
}

/// A line that is a measurement is a pdf dimension: never drawn this way.
/// A plain line with the same geometry is.
#[test]
fn a_dimension_line_is_declined_and_a_plain_line_is_drawn() {
    for extra in ["/IT /LineDimension", "/Cap true", "/LL 10"] {
        let out = one(&format!(
            "<< /Type /Annot /Subtype /Line /C [0 0 0] /L [50 150 250 150] {extra} \
             /Rect [40 140 260 160] >>"
        ));
        assert_eq!(ink(&out), 0, "{extra}: a pdf dimension stays blank");
        assert_eq!(drawn(&out, "Line"), 0, "{extra}");
    }
    let plain = one(
        "<< /Type /Annot /Subtype /Line /C [0 0 0] /L [50 150 250 150] \
         /Rect [40 140 260 160] >>",
    );
    assert!(ink(&plain) > 0);
    assert_eq!(drawn(&plain, "Line"), 1);
}

#[test]
fn an_annotation_that_states_no_colour_is_left_blank() {
    let out =
        one("<< /Type /Annot /Subtype /Ink /InkList [[50 50 250 250]] /Rect [40 40 260 260] >>");
    assert_eq!(ink(&out), 0);
    assert_eq!(drawn(&out, "Ink"), 0);
    assert_eq!(out.diagnostics.annotations_without_ap.get("Ink"), Some(&1));
}

#[test]
fn the_blank_setting_leaves_it_out() {
    let bytes = no_ap_pdf(
        "4 0 R",
        "<< /Type /Annot /Subtype /Ink /C [1 0 0] /InkList [[50 50 250 250]] \
         /Rect [40 40 260 260] >>",
        1,
    );
    let doc = Document::from_bytes(bytes).unwrap();
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    let options = RenderOptions::default().with_markup_without_ap(MarkupWithoutAp::Blank);
    let out = pdfcer_render::render_page_with(&doc, &pages[0], 1.0, &options).unwrap();
    assert_eq!(ink(&out), 0);
    assert_eq!(drawn(&out, "Ink"), 0);
}

/// The file's `/CA` applies to the drawn look as it would to an `/AP`.
#[test]
fn constant_opacity_applies() {
    let square = |ca: &str| {
        one(&format!(
            "<< /Type /Annot /Subtype /Square /IC [0 0 0] {ca} /Rect [40 40 260 260] >>"
        ))
    };
    assert_eq!(pixel(&square(""), 150, 150), (0, 0, 0));
    let (r, g, b) = pixel(&square("/CA 0.5"), 150, 150);
    assert!(
        (120..=135).contains(&r) && r == g && g == b,
        "half grey, got {r}"
    );
}

#[test]
fn polygon_polyline_and_circle_are_drawn() {
    for (subtype, geometry) in [
        ("Polygon", "/Vertices [50 50 250 50 150 250]"),
        ("PolyLine", "/Vertices [50 50 250 50 150 250]"),
        ("Circle", ""),
    ] {
        let out = one(&format!(
            "<< /Type /Annot /Subtype /{subtype} /C [1 0 0] {geometry} /Rect [40 40 260 260] >>"
        ));
        assert!(ink(&out) > 0, "{subtype}");
        assert_eq!(drawn(&out, subtype), 1, "{subtype}");
    }
}
