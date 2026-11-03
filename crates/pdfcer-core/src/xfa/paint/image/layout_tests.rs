//! A growable field or draw holding an image is laid at the image's
//! natural size.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // a panic here is the test failing

use super::tests::{base64, bmp};
use crate::xfa::{
    LayoutOptions, LayoutRect, XdpDocument, XfaImages, layout_template,
    layout_template_with_images, read_template,
};

const PAGE: &str = r#"<pageSet><pageArea name="P"><contentArea x="0pt" y="0pt" w="540pt" h="720pt"/></pageArea></pageSet>"#;

fn template(body: &str) -> crate::xfa::Template {
    let xdp = format!(
        r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><template xmlns="http://www.xfa.org/schema/xfa-template/3.3/"><subform name="f" layout="position">{PAGE}{body}</subform></template></xdp:xdp>"#
    );
    read_template(&XdpDocument::parse(xdp).unwrap())
        .unwrap()
        .unwrap()
}

fn first_rect(body: &str, opts: &LayoutOptions, images: &XfaImages) -> LayoutRect {
    let laid = layout_template_with_images(&template(body), opts, images).unwrap();
    laid.pages[0].items[0].rect
}

/// A growable draw holding a 20x10 px image that records no resolution.
fn draw() -> String {
    format!(
        r#"<draw name="d" x="0pt" y="0pt"><value><image>{}</image></value></draw>"#,
        base64(&bmp(20, 10, [255, 0, 0], 0))
    )
}

#[test]
fn an_inline_image_sizes_its_growable_draw_at_the_assumed_resolution() {
    let none = XfaImages::new();
    let r = first_rect(&draw(), &LayoutOptions::new(), &none);
    assert_eq!((r.w, r.h), (20.0, 10.0), "0 reads as 72 dpi");
    let r = first_rect(&draw(), &LayoutOptions::new().with_image_dpi(144), &none);
    assert_eq!((r.w, r.h), (10.0, 5.0));
    let given = draw().replace(r#"y="0pt">"#, r#"y="0pt" w="50pt" h="40pt">"#);
    let r = first_rect(&given, &LayoutOptions::new(), &none);
    assert_eq!((r.w, r.h), (50.0, 40.0), "a given size wins");
}

#[test]
fn margins_and_a_caption_are_added_to_the_image() {
    let field = format!(
        r#"<field name="f1" x="0pt" y="0pt"><ui><imageEdit/></ui><margin leftInset="2pt" rightInset="3pt" topInset="1pt" bottomInset="4pt"/><caption placement="left" reserve="30pt"><value><text>Photo</text></value></caption><value><image>{}</image></value></field>"#,
        base64(&bmp(20, 10, [255, 0, 0], 0))
    );
    let r = first_rect(&field, &LayoutOptions::new(), &XfaImages::new());
    assert_eq!((r.w, r.h), (55.0, 15.0));
}

#[test]
fn a_linked_image_is_measured_only_with_the_files() {
    let body =
        r#"<draw name="d" x="0pt" y="0pt" minW="7pt"><value><image href="a.bmp"/></value></draw>"#;
    let alone = layout_template(&template(body), &LayoutOptions::new()).unwrap();
    let r = alone.pages[0].items[0].rect;
    assert_eq!((r.w, r.h), (7.0, 0.0), "unresolved: minW, minH");
    let mut images = XfaImages::new();
    images.insert("a.bmp", bmp(30, 6, [0, 0, 255], 0));
    let r = first_rect(body, &LayoutOptions::new(), &images);
    assert_eq!((r.w, r.h), (30.0, 6.0));
}
