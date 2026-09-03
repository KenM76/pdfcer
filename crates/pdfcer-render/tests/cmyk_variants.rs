//! # CMYK/YCCK JPEG polarity — the decision 006 regression fixtures
//!
//! Pins the empirical conclusions of decision 006
//! (`docs/decisions/006-cmyk-jpeg-inversion.md` §3.4/§6.4) against the
//! six committed fixtures in `fixtures/synthetic/cmyk-variants/`
//! (provenance + construction: that directory's `PROVENANCE.md`; the
//! payload is CC BY 4.0 from the veraPDF corpus, byte-patched). All
//! six wrap the SAME 5,088-byte entropy-coded JPEG (300×232) at 1:1,
//! varying only the APP14 transform byte (2 / 0 / marker→COM) and the
//! presence of `/Decode [1 0 1 0 1 0 1 0]`.
//!
//! Two invariant families, each aimed at a specific silent-breakage
//! vector:
//!
//! 1. **Decoded CMYK sample values at named pixels.** pdfcer's YCCK
//!    inverse (`ycck_to_cmyk_in_place`, TN #5116 §13.1) assumes
//!    `zune-jpeg` returns raw, un-normalized YCC on a passthrough
//!    request — verified at 0.5.15, **not contractual** (006 §9
//!    revisit trigger 5). A silent upstream change to that
//!    passthrough, or an "Adobe inversion" creeping into any layer
//!    (forbidden by R29), moves these exact bytes.
//! 2. **Pixel-level render polarity for the `/Decode` variants.**
//!    `/Decode [1 0 …]` is the sole, sanctioned polarity control (R29,
//!    §8.9.5.2), implemented as a signed slope in
//!    `pdfcer-render/src/image.rs` — exactly where a well-meaning
//!    `min`/`max` "normalization" would silently destroy it. Each
//!    `_dec` render must come out dark where its plain twin is light.
//!
//! Ground truth for every expected value here is pdfium (via
//! `pypdfium2` page renders) — chosen per R31 after Pillow's
//! unconditional `CMYK;I` rawmode produced a false positive for this
//! exact investigation (006 §3.3). pdfcer matched pdfium on all six
//! variants when these constants were recorded (2026-07-31).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use pdfcer_core::document::Document;
use pdfcer_core::image_codec::{self, CodecColorModel, CodedImage};
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::page_tree;
use pdfcer_render::RenderedPage;

/// Load one of the six fixtures by file name.
fn fixture(name: &str) -> Document {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/cmyk-variants")
        .join(name);
    Document::from_bytes(std::fs::read(&path).expect("fixture file")).expect("fixture parses")
}

/// Decode the fixture's single image XObject (always object 5, per the
/// PROVENANCE construction note) through the public codec entry point,
/// bypassing rendering — this is what isolates invariant family 1 from
/// `/Decode`, `/ColorSpace` and colour conversion.
fn decode_samples(doc: &Document) -> CodedImage {
    let obj = doc.get(ObjId::new(5, 0)).expect("image object 5 0");
    let Object::Stream(stream) = &obj.value else {
        panic!("object 5 0 is not a stream");
    };
    let raw = stream.data_span.slice(doc.bytes()).expect("stream bytes");
    image_codec::decode_image(doc, &stream.dict, raw, false).expect("image decodes")
}

/// The four CMYK samples at pixel (x, y) of a 300-wide decode.
fn cmyk_at(img: &CodedImage, x: usize, y: usize) -> [u8; 4] {
    let i = (y * 300 + x) * 4;
    img.samples[i..i + 4].try_into().unwrap()
}

/// Render the fixture's only page at scale 1.0 — one sample per pixel,
/// no resampling allowance needed (PROVENANCE: image placed 1:1).
fn render(doc: &Document) -> RenderedPage {
    let pages = page_tree::pages(doc).expect("page tree");
    pdfcer_render::render_page(doc, &pages[0], 1.0).expect("renders")
}

/// Perceptually-weighted luminance of the rendered pixel at (x, y).
///
/// tiny-skia stores premultiplied RGBA; every pixel these tests probe
/// is fully opaque, so the premultiplied values ARE the colour values.
fn luminance_at(page: &RenderedPage, x: u32, y: u32) -> f32 {
    let px = page.pixmap.pixel(x, y).expect("pixel in bounds");
    0.299 * f32::from(px.red()) + 0.587 * f32::from(px.green()) + 0.114 * f32::from(px.blue())
}

// ---------------------------------------------------------------------------
// Invariant family 1 — decoded sample values at named pixels
// ---------------------------------------------------------------------------

#[test]
fn v2_ycck_inverse_recovers_true_ink_at_named_pixels() {
    // Decision 006 §3.2's probe values, byte-exact: the YCCK inverse
    // recovers TRUE ink directly (TN #5116 §13.1 defines the forward
    // transform on true ink; there is no further inversion step to
    // take, and R29 forbids inventing one). Pixel (2, 2) sits in the
    // white background — near-zero ink — and the centre of the
    // invader is cyan-dominant light blue.
    let img = decode_samples(&fixture("v2.pdf"));
    assert_eq!((img.width, img.height), (300, 232));
    assert_eq!(img.color_model, CodecColorModel::Cmyk);
    assert_eq!(
        cmyk_at(&img, 2, 2),
        [0, 3, 6, 0],
        "background must be near-zero ink (white) — an inverted decode reads ~[255,252,249,255]"
    );
    assert_eq!(
        cmyk_at(&img, 150, 116),
        [81, 25, 8, 1],
        "centre must be cyan-dominant (light blue invader)"
    );
    // Diagnostic classification (006 §4.4): transform 2 is the benign
    // census, never the R30 warning.
    assert!(img.notes.cmyk_image);
    assert!(!img.notes.cmyk_polarity_unverifiable);
}

#[test]
fn v0_passthrough_exposes_the_raw_stored_samples() {
    // With the transform byte patched to 0 the codec must NOT run the
    // YCCK inverse: the caller receives the stored (Y, Cb, Cr, K)
    // bytes verbatim. This is the value decision 006 §3.2 used to
    // prove the YCCK inverse correct (mapping [253,126,130,0] back to
    // [0,3,6,0] true ink), and it is the tripwire for any silent
    // change in zune-jpeg's input==output passthrough semantics.
    let img = decode_samples(&fixture("v0.pdf"));
    assert_eq!(img.color_model, CodecColorModel::Cmyk);
    assert_eq!(
        cmyk_at(&img, 2, 2),
        [253, 126, 130, 0],
        "raw stored (Y, Cb, Cr, K) — any transform applied here violates Table 13 / R29"
    );
    // Transform 0 with no /Decode is exactly the R30 shape.
    assert!(!img.notes.cmyk_image);
    assert!(img.notes.cmyk_polarity_unverifiable);
}

#[test]
fn vn_no_marker_defaults_to_transform_zero_and_matches_v0() {
    // APP14 rewritten to COM: no Adobe marker, no /DecodeParms, so
    // Table 13's default rule applies — "0 otherwise" for four
    // components — and the decode must be byte-identical to v0's.
    // (pdfium likewise applies no inversion on this branch: 006 §3.4.)
    let vn = decode_samples(&fixture("vn.pdf"));
    let v0 = decode_samples(&fixture("v0.pdf"));
    assert_eq!(
        vn.samples, v0.samples,
        "missing marker and explicit transform 0 must be the same passthrough"
    );
    assert!(vn.notes.cmyk_polarity_unverifiable);
}

#[test]
fn a_decode_array_never_reaches_the_codec_layer() {
    // R26 (as clarified by 006): the codec may OBSERVE /Decode for
    // diagnostics but never apply it — so each _dec variant's decoded
    // samples are byte-identical to its plain twin's, and only the
    // renderer's output differs (the tests below).
    for (plain, dec) in [
        ("v2.pdf", "v2_dec.pdf"),
        ("v0.pdf", "v0_dec.pdf"),
        ("vn.pdf", "vn_dec.pdf"),
    ] {
        let p = decode_samples(&fixture(plain));
        let d = decode_samples(&fixture(dec));
        assert_eq!(p.samples, d.samples, "{plain} vs {dec}");
        // The /Decode declaration also retires the R30 diagnostic —
        // the producer said what its samples mean (006 §4.3).
        assert!(!d.notes.cmyk_polarity_unverifiable, "{dec}");
    }
}

// ---------------------------------------------------------------------------
// Invariant family 2 — render polarity through /Decode (§8.9.5.2)
// ---------------------------------------------------------------------------

#[test]
fn v2_renders_light_and_its_decode_variant_renders_dark() {
    // The strong pair: v2 is the real corpus shape (light-blue invader
    // on WHITE), and /Decode [1 0 ×4] must invert it to near-black —
    // pdfium agrees on both (006 §3.4's matrix). A lost negative slope
    // in the §8.9.5.2 ramp makes these equal; an "Adobe inversion"
    // anywhere flips them.
    let plain = render(&fixture("v2.pdf"));
    let dec = render(&fixture("v2_dec.pdf"));
    assert_eq!(plain.diagnostics.images_rendered, 1);
    assert_eq!(plain.diagnostics.dct_cmyk_images, 1);
    assert_eq!(plain.diagnostics.dct_cmyk_polarity_unverifiable, 0);
    let bg_plain = luminance_at(&plain, 10, 10);
    let bg_dec = luminance_at(&dec, 10, 10);
    assert!(bg_plain > 200.0, "v2 background must be white: {bg_plain}");
    assert!(bg_dec < 60.0, "v2_dec background must be dark: {bg_dec}");
}

#[test]
fn every_decode_variant_is_darker_than_its_plain_twin() {
    // The full 006 §3.4 polarity matrix, one relation per pair. The
    // background of every plain variant is its lightest region (white
    // true ink for v2; the raw-YCC-as-CMYK teal for v0/vn), and the
    // [1 0 ×4] decode drives K to full on that background — so the
    // ordering is strict and large in all three columns.
    for (plain, dec) in [
        ("v2.pdf", "v2_dec.pdf"),
        ("v0.pdf", "v0_dec.pdf"),
        ("vn.pdf", "vn_dec.pdf"),
    ] {
        let l_plain = luminance_at(&render(&fixture(plain)), 10, 10);
        let l_dec = luminance_at(&render(&fixture(dec)), 10, 10);
        assert!(
            l_plain > l_dec + 50.0,
            "{plain} ({l_plain}) must render clearly lighter than {dec} ({l_dec})"
        );
    }
}

#[test]
fn vn_and_v0_render_identically() {
    // pdfium applies no inversion on the no-marker branch and neither
    // does pdfcer (006 §3.4: v0 and vn agree engine-by-engine). The
    // two rasters must be pixel-identical, not merely similar.
    let v0 = render(&fixture("v0.pdf"));
    let vn = render(&fixture("vn.pdf"));
    assert_eq!(v0.pixmap.data(), vn.pixmap.data());
}
