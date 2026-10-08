//! `replace_image` (pdfcer-gui request G156): a placed image's pixels are
//! replaced at the same point in the content stream, so its CTM and stacking
//! are kept, as one undo entry.

use std::path::Path;

use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditError, EditSession, ImageFit};
use pdfcer_core::image_import::{self, ImportedImage};
use pdfcer_core::object::ObjId;
use pdfcer_core::vector::{ImageObject, ImageSource, Matrix, VectorObject, decompose_page};
use pdfcer_core::writer::SaveOptions;

/// Objects: 0 a stroked path, 1 image XObject (60 × 40 at 10,20), 2 inline
/// image, 3 form, 4 image XObject rotated a quarter turn.
fn fixture() -> Vec<u8> {
    let page = "0 0 m 10 10 l S\nq 60 0 0 40 10 20 cm /Im1 Do Q\n\
                q 10 0 0 10 0 60 cm BI /W 1 /H 1 /BPC 8 /CS /G ID A EI Q\n\
                q 1 0 0 1 80 0 cm /Fm1 Do Q\nq 0 40 -60 0 200 100 cm /Im1 Do Q\n";
    let form = "0 0 10 10 re f\n";
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << /XObject \
         << /Im1 5 0 R /Fm1 6 0 R >> >> /Contents 4 0 R >>"
            .to_owned(),
        format!("<< /Length {} >>\nstream\n{page}endstream", page.len()),
        "<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 \
         /ColorSpace /DeviceGray /Length 1 >>\nstream\nA\nendstream"
            .to_owned(),
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Length {} >>\nstream\n{form}endstream",
            form.len()
        ),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objs.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = offsets.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

const OLD_IMAGE: ObjId = ObjId {
    num: 5,
    generation: 0,
};

fn session() -> EditSession {
    EditSession::new(Document::from_bytes(fixture()).unwrap())
}

fn picture(name: &str) -> ImportedImage {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/images")
        .join(name);
    image_import::import(&std::fs::read(path).unwrap()).unwrap()
}

fn reopened(s: &EditSession) -> Vec<VectorObject> {
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let doc = Document::from_bytes(bytes).unwrap();
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    decompose_page(&doc.view(), &pages[0], Matrix::IDENTITY)
        .unwrap()
        .objects
}

fn image(objs: &[VectorObject], i: usize) -> ImageObject {
    match &objs[i] {
        VectorObject::Image(img) => *img,
        other => panic!("object {i} is not an image: {other:?}"),
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn a_stretched_replacement_keeps_the_ctm_and_the_stacking() {
    let mut s = session();
    let before = reopened(&s);
    let depth = s.undo_kinds().len();
    let out = s
        .replace_image(0, 1, &picture("rgb8.png"), ImageFit::Stretch)
        .unwrap();
    assert_eq!(s.undo_kinds().len(), depth + 1);
    assert_eq!(out.replaced, Some(OLD_IMAGE));
    assert!(!out.disclosures.letterboxed);

    let after = reopened(&s);
    assert_eq!(after.len(), before.len(), "nothing added or removed");
    assert!(matches!(after[0], VectorObject::Path(_)), "stacking kept");
    let (old, new) = (image(&before, 1), image(&after, 1));
    assert_eq!(new.ctm, old.ctm);
    assert_eq!(new.xobject, Some(out.image_id));
    assert_eq!(new.pixel_size, Some((6, 4)));
    assert_eq!(
        image(&after, 4).xobject,
        Some(OLD_IMAGE),
        "the other placement still draws the old image"
    );

    assert_eq!(s.undo(), Some(CommandKind::ReplaceImage));
    assert_eq!(image(&reopened(&s), 1).xobject, Some(OLD_IMAGE));
}

#[test]
fn contain_centres_a_different_aspect_inside_the_old_extent() {
    let mut s = session();
    // A square picture in a 60 × 40 image: 40 × 40, centred horizontally.
    let out = s
        .replace_image(0, 1, &picture("icon32.png"), ImageFit::Contain)
        .unwrap();
    assert!(out.disclosures.letterboxed);
    let b = image(&reopened(&s), 1).page_bbox;
    assert!(
        close(b.min.x, 20.0)
            && close(b.max.x, 60.0)
            && close(b.min.y, 20.0)
            && close(b.max.y, 60.0),
        "{b:?}"
    );
    // The same picture filling the rotated placement's 40 × 60 extent.
    s.replace_image(0, 4, &picture("icon32.png"), ImageFit::Contain)
        .unwrap();
    let b = image(&reopened(&s), 4).page_bbox;
    assert!(
        close(b.max.x - b.min.x, 40.0) && close(b.max.y - b.min.y, 40.0),
        "{b:?}"
    );
    assert!(b.min.x >= 140.0 - 1e-6 && b.max.x <= 200.0 + 1e-6, "{b:?}");
    assert!(b.min.y >= 100.0 - 1e-6 && b.max.y <= 140.0 + 1e-6, "{b:?}");
}

#[test]
fn an_inline_image_becomes_an_xobject() {
    let mut s = session();
    let out = s
        .replace_image(0, 2, &picture("gray8.png"), ImageFit::Stretch)
        .unwrap();
    assert_eq!(out.replaced, None);
    let new = image(&reopened(&s), 2);
    assert_eq!(new.source, ImageSource::XObject);
    assert_eq!(new.xobject, Some(out.image_id));
    assert!(close(new.page_bbox.max.x, 10.0) && close(new.page_bbox.max.y, 70.0));
}

#[test]
fn paths_forms_and_bad_indices_are_refused() {
    let mut s = session();
    let depth = s.undo_kinds().len();
    let img = picture("rgb8.png");
    for (index, want) in [(0, "path"), (3, "form")] {
        let err = s
            .replace_image(0, index, &img, ImageFit::Contain)
            .unwrap_err();
        assert!(
            matches!(err, EditError::ReplaceImageOnOther { kind, .. } if kind == want),
            "{err:?}"
        );
    }
    assert!(s.replace_image(0, 99, &img, ImageFit::Contain).is_err());
    assert!(matches!(
        s.replace_image(5, 1, &img, ImageFit::Contain).unwrap_err(),
        EditError::PageOutOfRange { .. }
    ));
    assert_eq!(s.undo_kinds().len(), depth);
}
