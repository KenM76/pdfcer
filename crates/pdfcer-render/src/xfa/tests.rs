#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // a panic here is the test failing

use super::*;
use pdfcer_core::document::Document;
use pdfcer_core::xfa::{
    LayoutOptions, MissingFill, PaintNoteKind, XdpDocument, layout_template, read_template,
};

const PAGE: &str = r#"<pageSet><pageArea name="P"><contentArea x="0pt" y="0pt" w="100pt" h="100pt"/><medium short="100pt" long="100pt"/></pageArea></pageSet>"#;

fn template(body: &str) -> Template {
    let xdp = format!(
        r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><template xmlns="http://www.xfa.org/schema/xfa-template/3.3/"><subform name="f">{PAGE}{body}</subform></template></xdp:xdp>"#
    );
    read_template(&XdpDocument::parse(xdp).unwrap())
        .unwrap()
        .unwrap()
}

/// A one-page PDF; the XFA page does not read it.
fn doc() -> Document {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>",
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, b) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{b}\nendobj\n", i + 1).as_bytes());
    }
    let xref = buf.len();
    buf.extend_from_slice(b"xref\n0 4\n0000000000 65535 f\r\n");
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    Document::from_bytes(buf).unwrap()
}

fn render(body: &str, paint: &PaintOptions) -> RenderedXfaPage {
    render_with(body, paint, &RenderOptions::default())
}

fn render_with(body: &str, paint: &PaintOptions, options: &RenderOptions) -> RenderedXfaPage {
    let t = template(body);
    let l = layout_template(&t, &LayoutOptions::new()).unwrap();
    let d = doc();
    render_xfa_page(&d.view(), &t, &l.pages[0], paint, 1.0, options).unwrap()
}

fn rgb(p: &RenderedXfaPage, x: u32, y: u32) -> [u8; 3] {
    let px = p.page.pixmap.pixel(x, y).unwrap().demultiply();
    [px.red(), px.green(), px.blue()]
}

#[test]
fn a_filled_field_lands_at_its_layout_position_y_down() {
    // A red box at x=10..30, y=20..40 in XFA's y-down page space.
    let out = render(
        r#"<field name="f1" x="10pt" y="20pt" w="20pt" h="20pt"><border><edge presence="hidden"/><fill><color value="255,0,0"/></fill></border></field>"#,
        &PaintOptions::new(),
    );
    assert_eq!(out.page.pixmap.width(), 100);
    assert_eq!(rgb(&out, 20, 30), [255, 0, 0]);
    assert_eq!(rgb(&out, 20, 70), [255, 255, 255], "not flipped to y-up");
    assert_eq!(rgb(&out, 5, 5), [255, 255, 255]);
}

#[test]
fn the_paint_settings_and_notes_reach_the_caller() {
    let body = r#"<field name="f1" x="10pt" y="10pt" w="40pt" h="40pt"><border><edge presence="hidden"/></border><caption/></field>"#;
    let clear = RenderOptions::default().with_backdrop(PageBackdrop::Transparent);
    let none = render_with(body, &PaintOptions::new(), &clear);
    let white = render_with(
        body,
        &PaintOptions::new().with_missing_fill(MissingFill::White),
        &clear,
    );
    let alpha = |p: &RenderedXfaPage| p.page.pixmap.pixel(30, 30).unwrap().alpha();
    assert_eq!(alpha(&none), 0, "no fill element draws nothing by default");
    assert_eq!(alpha(&white), 255, "MissingFill::White paints the box");
    assert!(none.notes.iter().any(|n| n.kind == PaintNoteKind::Content));
}

#[test]
fn a_page_too_large_for_the_raster_is_refused() {
    let t = template("");
    let mut l = layout_template(&t, &LayoutOptions::new()).unwrap();
    l.pages[0].width = 1.0e9;
    let d = doc();
    let r = render_xfa_page(
        &d.view(),
        &t,
        &l.pages[0],
        &PaintOptions::new(),
        1.0,
        &RenderOptions::default(),
    );
    assert!(matches!(r, Err(RenderError::BadRasterSize { .. })));
}

#[test]
fn value_text_reaches_the_raster_through_the_painted_font_resources() {
    let body = r#"<draw name="d" x="0pt" y="0pt" w="100pt" h="100pt"><font typeface="Helvetica" size="40pt"/><value><text>WW</text></value></draw>"#;
    let out = render(body, &PaintOptions::new());
    let dark = (0..100)
        .flat_map(|y| (0..100).map(move |x| (x, y)))
        .filter(|&(x, y)| rgb(&out, x, y)[0] < 128)
        .count();
    assert!(dark > 200, "only {dark} dark pixels");
    assert_eq!(out.page.diagnostics.unknown_ops, 0);
    assert_eq!(out.page.diagnostics.deferred_ops, 0);
}
