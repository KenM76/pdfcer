//! `ImportedImage::from_rgba8`: in-memory pixels give the picture a PNG of
//! the same pixels gives through `image_import::import`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::filters::flate;
use pdfcer_core::image_import::{
    self, ImageFormat, ImageImportError, ImportColorSpace, ImportedImage,
};
use std::path::Path;

fn rgba8_png() -> ImportedImage {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/images/rgba8.png");
    image_import::import(&std::fs::read(path).unwrap()).unwrap()
}

/// The image's base samples and soft-mask samples, inflated.
fn halves(img: &ImportedImage) -> (Vec<u8>, Option<Vec<u8>>) {
    let base = flate::decode(&img.data, None).unwrap();
    let mask = img
        .soft_mask
        .as_ref()
        .map(|m| flate::decode(&m.data, None).unwrap());
    (base, mask)
}

fn interleave(rgb: &[u8], alpha: &[u8]) -> Vec<u8> {
    rgb.chunks_exact(3)
        .zip(alpha)
        .flat_map(|(c, &a)| [c[0], c[1], c[2], a])
        .collect()
}

#[test]
fn translucent_pixels_match_the_png_route() {
    let png = rgba8_png();
    let (rgb, mask) = halves(&png);
    let mask = mask.expect("rgba8.png carries a soft mask");
    assert!(
        mask.iter().any(|&a| a != 255),
        "fixture must be translucent"
    );

    let px = ImportedImage::from_rgba8(png.width, png.height, &interleave(&rgb, &mask)).unwrap();
    assert_eq!(px.format, ImageFormat::Pixels);
    assert_eq!(px.color_space, ImportColorSpace::DeviceRgb);
    assert_eq!(px.bits_per_component, 8);
    assert_eq!(halves(&px), (rgb, Some(mask)));
    let sm = px.soft_mask.as_ref().unwrap();
    assert_eq!(
        (sm.width, sm.height, sm.bits_per_component),
        (png.width, png.height, 8)
    );
    assert_eq!(px.notes.recompressed, png.notes.recompressed);
    assert_eq!(px.notes.alpha_to_soft_mask, png.notes.alpha_to_soft_mask);
    assert_eq!(
        px.notes.requires_pdf_version,
        png.notes.requires_pdf_version
    );
}

#[test]
fn opaque_pixels_have_no_soft_mask_and_no_notes() {
    let png = rgba8_png();
    let (rgb, _) = halves(&png);
    let opaque = vec![255; (png.width * png.height) as usize];
    let px = ImportedImage::from_rgba8(png.width, png.height, &interleave(&rgb, &opaque)).unwrap();
    assert!(px.soft_mask.is_none());
    assert_eq!(px.notes, Default::default());
    assert_eq!(halves(&px).0, rgb);
}

#[test]
fn a_wrong_length_buffer_is_refused_by_name() {
    let err = ImportedImage::from_rgba8(2, 2, &[0; 15]).unwrap_err();
    assert_eq!(
        err,
        ImageImportError::BufferSize {
            width: 2,
            height: 2,
            expected: 16,
            actual: 15
        }
    );
    assert!(err.to_string().contains("needs 16 bytes"));
}

#[test]
fn an_empty_image_is_refused() {
    assert!(matches!(
        ImportedImage::from_rgba8(0, 3, &[]),
        Err(ImageImportError::Empty {
            width: 0,
            height: 3
        })
    ));
}
