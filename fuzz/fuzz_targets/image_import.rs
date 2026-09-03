//! Fuzz target: **raster-image import**
//! (`pdfcer_core::image_import::import_with`, all four compression policies).
//!
//! ## Why this target exists now
//!
//! `image_import` has parsed attacker-controlled PNG, JPEG and BMP
//! containers since it landed, and until 2026-08-08 every one of its
//! branches either copied bytes verbatim or ran them through a decoder that
//! is fuzzed elsewhere. That is no longer true:
//! [`ImageCompression::Jpeg`](pdfcer_core::image_import::ImageCompression::Jpeg)
//! added a path that **decodes attacker-controlled input, reshapes the
//! samples in pdfcer's own hand-written code, and feeds the result to an
//! encoder**. The reshaping is the new attack surface, and it is exactly the
//! kind of code — strides, sub-byte bit packing, palette indices, a `u16`
//! dimension API — that the project's own §10 posture says must be fuzzed
//! rather than reasoned about.
//!
//! ## What is actually being aimed at
//!
//! The container parsers and the codecs have their own targets
//! (`image_codec_dct`, `image_codec_ccitt`, `image_codec_jbig2`,
//! `image_codec_jpx`), and `zune-jpeg`/`hayro-*` carry upstream campaigns.
//! Per decision 005 §6.5's "fuzz the glue, not the vendor's core", the
//! interesting failures here are pdfcer's own:
//!
//! 1. **The row-stride walk** in `jpeg_encode::unpack_to_bytes`. `stride`,
//!    `width`, `height` and the component count come from four separately
//!    parsed places; a row index times a stride is a multiplication of
//!    attacker-chosen values, and the last row of a truncated buffer is the
//!    classic off-by-one.
//! 2. **Sub-byte unpacking.** 1/2/4-bit samples are read high-order-bit
//!    first with a computed shift. A shift derived from a bit depth the
//!    container declared is a shift an attacker chose.
//! 3. **Palette expansion.** §8.6.6.3's `hival` and the lookup table's
//!    length are independent parsed values; every index is a slice offset.
//! 4. **The `u16` dimension boundary.** The encoder's API is 16-bit while
//!    pdfcer's ceilings are 32-bit. The conversion is checked rather than
//!    cast, and this target is what proves the check is reachable and
//!    correct rather than decorative.
//! 5. **Geometry agreement.** The sample buffer's length is validated
//!    against `width × height × components` before the encoder sees it, so
//!    a decoder that returns a short buffer becomes a named error rather
//!    than a panic inside a third-party crate.
//!
//! ## Invariant asserted
//!
//! For ANY input and ANY policy, `import_with` returns `Ok(_)` or a
//! structured [`ImageImportError`](pdfcer_core::image_import::ImageImportError)
//! — never a panic, never an abort, never an unbounded allocation. On
//! success, the returned image's own fields must agree with each other:
//! a `/DCTDecode` result is always 8 bits per component (Table 89), and a
//! non-empty image always has non-zero dimensions. An inconsistent
//! `ImportedImage` is a bug even when nothing panicked, because
//! `EditSession::add_image` writes those fields into a dictionary that
//! describes bytes it does not re-check.
//!
//! ## The cost gate, and why it is not a weakening
//!
//! `MAX_IMAGE_PIXELS` is 32 Mpx. Encoding a 32-megapixel image on every
//! iteration would give libFuzzer roughly one execution per second and the
//! campaign would explore nothing. So the two expensive policies (`Lossless`
//! and `Jpeg`) run only when the cheap passthrough import has already
//! revealed a modest pixel count.
//!
//! This does not hide the ceiling logic: the ceiling is enforced in
//! `check_dimensions`, *before* any of this, and it is exercised on every
//! iteration by the passthrough import that the gate reads its answer from.
//! What the gate skips is re-running an encoder over a large but already
//! well-formed buffer, which is throughput, not coverage.
//!
//! Seed corpora come from `fixtures/synthetic/images/` only, never from a
//! downloaded real-world image (`docs/LEGAL.md` §5).

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::image_import::{self, ImageCompression, ImportFilter, ImportOptions};

/// Above this many pixels, the re-encoding policies are skipped for
/// throughput. See the module docs — this is a speed gate, not a safety one.
const REENCODE_PIXEL_BUDGET: u64 = 1 << 18; // 262 144 px, e.g. 512 × 512

/// Assert the internal agreement an `ImportedImage` promises its consumer.
fn check(img: &image_import::ImportedImage) {
    assert!(
        img.width > 0 && img.height > 0,
        "an imported image has pixels"
    );
    if img.filter == ImportFilter::DctDecode {
        // §7.4.8 / Table 89: DCTDecode "shall always deliver 8-bit samples".
        assert_eq!(img.bits_per_component, 8, "DCT is 8-bit, always");
    }
    assert!(
        matches!(img.bits_per_component, 1 | 2 | 4 | 8 | 16),
        "Table 89 admits only 1, 2, 4, 8, 16"
    );
    if let Some(mask) = &img.soft_mask {
        // The conservative shape the module documents: identical dimensions.
        assert_eq!((mask.width, mask.height), (img.width, img.height));
    }
    // `stored_bytes` is what a front end shows the operator; it must count
    // what is actually there rather than a stale figure from a branch that
    // rewrote `data`.
    let expect = img.data.len() + img.soft_mask.as_ref().map_or(0, |m| m.data.len());
    assert_eq!(
        img.notes.stored_bytes, expect,
        "the reported size is the real one"
    );
}

fuzz_target!(|data: &[u8]| {
    // Policy 1 of 4: the default. Always run — it is cheap, it is what most
    // callers get, and its result gates the expensive policies below.
    let base = match image_import::import(data) {
        Ok(img) => {
            check(&img);
            img
        }
        // Every refusal is structured by construction (the function returns
        // `Result<_, ImageImportError>`); reaching here means the input was
        // refused, which is a correct outcome, not a finding.
        Err(_) => return,
    };

    // Both re-encoding policies decode the image, so they are bounded by the
    // pixel count rather than by the input length — a 200-byte PNG can
    // declare a 10 000 × 10 000 canvas.
    let pixels = u64::from(base.width) * u64::from(base.height);
    if pixels > REENCODE_PIXEL_BUDGET {
        return;
    }

    // Policy 2: lossless storage of the decoded samples.
    if let Ok(img) = image_import::import_with(
        data,
        &ImportOptions::new().with_compression(ImageCompression::Lossless),
    ) {
        check(&img);
    }

    // Policies 3 and 4: the re-encode, at both ends of the quality range.
    // Both endpoints, because the encoder's chroma-subsampling default is
    // quality-conditioned (it switches at 90), so a single quality would
    // leave one of its two component layouts entirely unexplored.
    for quality in [1u8, 100] {
        if let Ok(img) = image_import::import_with(
            data,
            &ImportOptions::new().with_compression(ImageCompression::Jpeg { quality }),
        ) {
            check(&img);
            assert_eq!(
                img.filter,
                ImportFilter::DctDecode,
                "the jpeg policy writes DCT"
            );
            assert_eq!(img.notes.jpeg_quality, Some(quality));
        }
    }
});
