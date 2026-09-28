//! `Page::with_boxes` (`G060`) builds, from outside the crate, the same
//! page the page-tree walk resolves for a page stating no production boxes.

use pdfcer_core::document::Document;
use pdfcer_core::object::ObjId;
use pdfcer_core::page_tree::{BoxResolution, Page, Rect, pages_in};

fn one_page(rotate: i32) -> Document {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
             /CropBox [10 20 600 780] /Rotate {rotate} /Resources << >> >>"
        ),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 4\n0000000000 65535 f\r\n");
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    Document::from_bytes(buf).expect("loads")
}

#[test]
fn with_boxes_matches_the_page_tree_for_a_page_without_production_boxes() {
    let doc = one_page(-270);
    let walked = pages_in(&doc.view()).expect("walks").remove(0);
    let media = Rect::from_corners(0.0, 0.0, 612.0, 792.0);
    let crop = Rect::from_corners(10.0, 20.0, 600.0, 780.0);
    let built = Page::with_boxes(ObjId::new(3, 0), media, crop, 90);
    assert_eq!(built.id, walked.id);
    assert_eq!(built.rotate, walked.rotate);
    assert_eq!(built.media_box, walked.media_box);
    for (b, w) in [
        (built.crop_box, walked.crop_box),
        (built.bleed_box, walked.bleed_box),
        (built.trim_box, walked.trim_box),
        (built.art_box, walked.art_box),
    ] {
        assert_eq!(b, w);
    }
    // The walk says `AsWritten` for the crop box the file wrote; the
    // constructor's page wrote none.
    assert_eq!(walked.crop_box_resolution, BoxResolution::AsWritten);
    for r in [
        built.crop_box_resolution,
        built.bleed_box_resolution,
        built.trim_box_resolution,
        built.art_box_resolution,
        walked.bleed_box_resolution,
        walked.trim_box_resolution,
        walked.art_box_resolution,
    ] {
        assert_eq!(r, BoxResolution::Defaulted);
    }
    assert_eq!(built.resources, walked.resources);
    assert_eq!(built.contents, walked.contents);
    assert_eq!(built.resources_defaulted, walked.resources_defaulted);
}

#[test]
fn with_boxes_normalizes_rotation_and_takes_struct_update() {
    let r = Rect::from_corners(0.0, 0.0, 100.0, 100.0);
    let id = ObjId::new(1, 0);
    assert_eq!(Page::with_boxes(id, r, r, 630).rotate, 270);
    assert_eq!(Page::with_boxes(id, r, r, 45).rotate, 0);
    let p = Page {
        contents: vec![ObjId::new(9, 0)],
        ..Page::with_boxes(id, r, r, 0)
    };
    assert_eq!(p.contents, [ObjId::new(9, 0)]);
    assert_eq!((p.contents_unresolved, p.contents_flattened), (0, 0));
}
