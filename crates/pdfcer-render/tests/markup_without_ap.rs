//! Decision 203: a markup annotation with no `/AP` is drawn from its own
//! geometry and colours when the page renders, and left blank (with a note)
//! when that would need guessing. Nothing is written to the file.

use crate::named_icon_without_ap::{ink, no_ap_pdf, no_ap_pdf_with_catalog, render};
use pdfcer_core::document::Document;
use pdfcer_render::{FreeTextC, MarkupWithoutAp, RenderOptions, RenderedPage};

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

// --- /FreeText (Pass 646.22) ---------------------------------------------

const DR_TIMES: &str = "/AcroForm << /Fields [] /DR << /Font << /Helv << /Type /Font \
     /Subtype /Type1 /BaseFont /Times-Roman >> >> >> >>";

fn free_text(extra: &str) -> String {
    format!(
        "<< /Type /Annot /Subtype /FreeText /Contents (Hello) /DA (/Helv 12 Tf 0 g) {extra} \
         /Rect [40 40 260 120] >>"
    )
}

fn notes(out: &RenderedPage) -> String {
    out.diagnostics.annotation_notes.join("\n")
}

fn render_with(bytes: Vec<u8>, options: &RenderOptions) -> RenderedPage {
    let doc = Document::from_bytes(bytes).unwrap();
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    pdfcer_render::render_page_with(&doc, &pages[0], 1.0, options).unwrap()
}

#[test]
fn a_free_text_is_drawn_and_its_font_substitution_disclosed() {
    let out = one(&free_text(""));
    assert!(ink(&out) > 0, "the text is on the page");
    assert_eq!(drawn(&out, "FreeText"), 1);
    assert_eq!(out.diagnostics.annotations_painted, 0);
    assert!(
        notes(&out).contains("drawn in Helvetica"),
        "no /DR: the substitute is named, got {}",
        notes(&out)
    );
}

/// The `/DR` font is the one drawn: no substitution note, and a different
/// picture from the Helvetica substitute.
#[test]
fn a_dr_font_is_used_and_not_called_a_substitute() {
    let with_dr = render(no_ap_pdf_with_catalog(DR_TIMES, "4 0 R", &free_text(""), 1));
    assert_eq!(drawn(&with_dr, "FreeText"), 1);
    assert!(!notes(&with_dr).contains("drawn in Helvetica"));
    assert_ne!(with_dr.pixmap.data(), one(&free_text("")).pixmap.data());
}

#[test]
fn c_fills_by_default_and_strokes_the_border_when_set() {
    let bytes = || no_ap_pdf("4 0 R", &free_text("/C [1 0 0]"), 1);
    let fill = render(bytes());
    assert_eq!(pixel(&fill, 200, 50), (255, 0, 0), "/C is the fill");
    assert!(notes(&fill).contains("/C read as the fill"));

    let border = render_with(
        bytes(),
        &RenderOptions::default().with_free_text_c(FreeTextC::Border),
    );
    assert_eq!(pixel(&border, 200, 50), (255, 255, 255), "no fill");
    assert_ne!(
        pixel(&border, 40, 80),
        (255, 255, 255),
        "the border is drawn"
    );
}

/// `/IC` is pdfcer's fill key, so `/C` is the border whatever the setting.
#[test]
fn ic_makes_c_the_border() {
    let out = one(&free_text("/C [1 0 0] /IC [0 0 1]"));
    assert_eq!(pixel(&out, 200, 50), (0, 0, 255));
    assert!(!notes(&out).contains("/C read as the fill"));
}

#[test]
fn a_callout_rich_only_or_daless_free_text_is_declined() {
    for (annot, why) in [
        (
            free_text("/IT /FreeTextCallout /CL [10 10 40 40]"),
            "callout",
        ),
        (
            "<< /Type /Annot /Subtype /FreeText /RC (<body>Hi</body>) /DA (/Helv 12 Tf 0 g) \
             /Rect [40 40 260 120] >>"
                .to_owned(),
            "rich text only",
        ),
        (
            "<< /Type /Annot /Subtype /FreeText /Contents (Hi) /Rect [40 40 260 120] >>".to_owned(),
            "DA",
        ),
    ] {
        let out = one(&annot);
        assert_eq!(ink(&out), 0, "{why}");
        assert_eq!(drawn(&out, "FreeText"), 0, "{why}");
        assert!(notes(&out).contains(why), "{why}: {}", notes(&out));
    }
}

/// Table 174 `/RD`: the box is drawn in `/Rect` less the differences.
#[test]
fn rd_insets_the_free_text_box() {
    let out = one(&free_text("/C [1 0 0] /RD [20 10 20 10]"));
    assert_eq!(
        pixel(&out, 50, 80),
        (255, 255, 255),
        "outside the inner box"
    );
    assert_eq!(pixel(&out, 200, 55), (255, 0, 0), "inside it");
}
