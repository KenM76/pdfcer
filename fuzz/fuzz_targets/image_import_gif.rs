//! Fuzz target: **GIF import** (`pdfcer_core::image_import::import` on GIF
//! input, which routes to `image_import::gif`).
//!
//! The LZW codec is weezl's; what is aimed at here is pdfcer's own glue: the
//! block walk and sub-block reader, colour-table sizing, the interlace row
//! map, compositing a frame at an offset onto the logical screen, and the
//! frame counter that skips later images undecoded.
//!
//! ## Invariant asserted
//!
//! Any input returns `Ok(_)` or a structured `ImageImportError`, never a
//! panic or an unbounded allocation. On success the image is self-consistent:
//! `/Indexed` 8-bit with a `3 × (hival + 1)` lookup, `width × height` indices
//! none past `hival`, and a soft mask that is present exactly when some pixel
//! is clear, holding only 0 and 255.
//!
//! Seeds are `fixtures/synthetic/gif/` only (`docs/LEGAL.md` §5).

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::filters::flate;
use pdfcer_core::image_import::{self, ImageFormat, ImportColorSpace};

/// Above this many pixels the streams are not re-inflated, for throughput;
/// the dimension ceiling is enforced before any of this either way.
const INSPECT_PIXEL_BUDGET: usize = 1 << 18;

fuzz_target!(|data: &[u8]| {
    let Ok(img) = image_import::import(data) else {
        return;
    };
    if img.format != ImageFormat::Gif {
        return;
    }
    assert_eq!(img.bits_per_component, 8);
    let ImportColorSpace::Indexed { hival, lookup } = &img.color_space else {
        panic!("a GIF imports as /Indexed");
    };
    assert_eq!(lookup.len(), 3 * (usize::from(*hival) + 1));
    assert!(img.width > 0 && img.height > 0);

    let pixels = img.width as usize * img.height as usize;
    if pixels > INSPECT_PIXEL_BUDGET {
        return;
    }
    let indices = flate::decode(&img.data, None).expect("pdfcer's own Flate");
    assert_eq!(indices.len(), pixels);
    assert!(
        indices.iter().all(|i| i <= hival),
        "an index past the table"
    );
    match &img.soft_mask {
        Some(mask) => {
            assert_eq!((mask.width, mask.height), (img.width, img.height));
            let alpha = flate::decode(&mask.data, None).expect("pdfcer's own Flate");
            assert_eq!(alpha.len(), pixels);
            assert!(alpha.iter().all(|&a| a == 0 || a == 255));
            assert!(alpha.contains(&0), "a mask is written only when needed");
            assert!(img.notes.alpha_to_soft_mask);
        }
        None => assert!(!img.notes.alpha_to_soft_mask),
    }
});
